//! Attribute types and casting
//!
//! This module groups custom field types and attribute-casting helpers.
//!
//! Use these types when a plain Rust scalar is not enough for the storage or
//! serialization behavior you need, such as encrypted values, hashes, JSON, or
//! database enums.
//!
//! If a field loads or saves with the wrong representation, this module is the
//! first place to check.
//!
//! Practical split:
//! - use `Encrypted` and `Hashed` when the stored value should not round-trip as plain text
//! - use the JSON aliases when the database representation is more complex than a scalar
//! - check `cast` first when a value is being transformed unexpectedly during load or save

mod aliases;
mod cast;
mod encrypted;
mod hashed;
mod timestamps;

pub use aliases::{
    BigIntArray, BoolArray, DateTime, Decimal, FloatArray, IntArray, Json, JsonArray, Jsonb,
    NaiveDate, NaiveDateTime, NaiveTime, Text, TextArray, Utc, Uuid,
};
pub use cast::{CastType, CastValue, Castable};
pub use encrypted::Encrypted;
pub use hashed::Hashed;
pub use timestamps::{UnixTimestamp, UnixTimestampMillis};

#[cfg(all(test, feature = "encrypted-fields"))]
pub(crate) use encrypted::encrypt_json_value as __encrypt_json_value;
#[cfg(test)]
pub(crate) use encrypted::encrypted_field_missing_key_error;
#[cfg(feature = "encrypted-fields")]
pub(crate) use encrypted::{
    decrypt_json_value_for_attribute as __decrypt_json_value_for_attribute,
    encrypt_json_value_for_attribute as __encrypt_json_value_for_attribute,
    is_encrypted_json_value as __is_encrypted_json_value,
};

#[cfg(test)]
#[path = "../../tests/unit/types_tests.rs"]
mod tests;
