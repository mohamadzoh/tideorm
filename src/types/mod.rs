//! Field value types.
//!
//! Re-exported value types (`DateTime`, `Decimal`, `Uuid`, ...), column type
//! aliases (`Json`, `Text`, the PostgreSQL array aliases), the one-way
//! [`Hashed`] password type, and the Unix timestamp helpers.
//!
//! Column encryption is not a type: mark the field `#[tideorm(encrypted)]`
//! (feature `encrypted-fields`) and keep the plain Rust type. See the `encrypted`
//! module for the payload format.

mod aliases;
#[cfg(feature = "encrypted-fields")]
pub(crate) mod encrypted;
mod hashed;
mod timestamps;

pub use aliases::{
    BigIntArray, BoolArray, DateTime, Decimal, FloatArray, IntArray, Json, JsonArray, Jsonb,
    NaiveDate, NaiveDateTime, NaiveTime, Text, TextArray, Utc, Uuid,
};
pub use hashed::Hashed;
pub use timestamps::{UnixTimestamp, UnixTimestampMillis};

#[cfg(test)]
#[path = "../../tests/unit/types_tests.rs"]
mod tests;
