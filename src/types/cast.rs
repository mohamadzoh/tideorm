use std::fmt;

use super::Uuid;

/// Trait for types that can be cast to/from database values
pub trait Castable: Sized {
    /// Cast from a serde_json::Value
    fn from_json(value: &serde_json::Value) -> Result<Self, String>;

    /// Cast to a serde_json::Value
    fn to_json(&self) -> serde_json::Value;
}

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
            .and_then(|arr| arr.iter().map(T::from_json).collect::<Result<Vec<_>, _>>())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(self.iter().map(|v| v.to_json()).collect())
    }
}

/// Trait for attribute casters that transform values when reading/writing
pub trait AttributeCaster<T>: Sized {
    /// Cast from database value to Rust type
    fn get(value: serde_json::Value) -> Result<T, String>;

    /// Cast from Rust type to database value
    fn set(value: &T) -> serde_json::Value;
}

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
            CastType::Json => Ok(value.clone()),
            CastType::Array | CastType::Collection => match value {
                serde_json::Value::Array(_) => Ok(value.clone()),
                serde_json::Value::String(s) => serde_json::from_str(s).or_else(|_| {
                    Ok(serde_json::Value::Array(
                        s.split(',')
                            .map(|v| serde_json::Value::String(v.trim().to_string()))
                            .collect(),
                    ))
                }),
                serde_json::Value::Null => Ok(serde_json::Value::Array(vec![])),
                _ => Err("Cannot cast to array".to_string()),
            },
            CastType::DateTime => match value {
                serde_json::Value::String(s) => {
                    if chrono::DateTime::parse_from_rfc3339(s).is_ok() {
                        Ok(value.clone())
                    } else {
                        Err("Invalid datetime format".to_string())
                    }
                }
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Err("Cannot cast to datetime".to_string()),
            },
            CastType::Date => match value {
                serde_json::Value::String(s) => {
                    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
                        Ok(value.clone())
                    } else {
                        Err("Invalid date format".to_string())
                    }
                }
                serde_json::Value::Null => Ok(serde_json::Value::Null),
                _ => Err("Cannot cast to date".to_string()),
            },
            CastType::Time => match value {
                serde_json::Value::String(s) => {
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
            },
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
            CastType::Encrypted | CastType::Hashed => Ok(value.clone()),
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
            CastType::Custom => Ok(value.clone()),
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
