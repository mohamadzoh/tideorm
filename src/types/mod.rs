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

mod aliases;
mod cast;
mod collections;
mod db_enum;
mod defaults;
mod encrypted;
mod hashed;
mod timestamps;

pub use aliases::{
    BigIntArray, BoolArray, DateTime, Decimal, FloatArray, IntArray, Json, JsonArray, Jsonb,
    NaiveDate, NaiveDateTime, NaiveTime, Text, TextArray, Utc, Uuid,
};
pub use cast::{AttributeCaster, CastType, CastValue, Castable};
pub use collections::{Collection, CommaSeparated};
pub use db_enum::DbEnum;
pub use defaults::{Accessor, Mutator, WithDefault};
pub use encrypted::Encrypted;
pub use hashed::Hashed;
pub use timestamps::{UnixTimestamp, UnixTimestampMillis};

#[cfg(test)]
pub(crate) use encrypted::encrypted_field_missing_key_error;

#[cfg(test)]
#[path = "../testing/types_tests.rs"]
mod tests;
