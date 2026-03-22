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
