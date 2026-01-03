//! Attribute types and casting
//!
//! This module provides type definitions and casting utilities for model attributes.

use serde::{Deserialize, Serialize};
use std::fmt;

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
        value.as_str().map(|s| s.to_string()).ok_or_else(|| "Expected string".to_string())
    }
    
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.clone())
    }
}

impl Castable for i32 {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value.as_i64().map(|n| n as i32).ok_or_else(|| "Expected integer".to_string())
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
        value.as_bool().ok_or_else(|| "Expected boolean".to_string())
    }
    
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Bool(*self)
    }
}

impl Castable for Uuid {
    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        value.as_str()
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
        value.as_array()
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
