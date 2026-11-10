//! Bind filter and assignment values with the type of the column they target.
//!
//! A filter value of a type the column holds binds as itself. A JSON value,
//! or one of another type, is read as the column's type: a UUID or a
//! timestamp written as text, bound as text, is rejected by PostgreSQL
//! (`uuid = text`) and silently matches nothing on SQLite, which stores UUIDs
//! as 16-byte blobs, and on MySQL's `BINARY(16)`.

use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use super::{Value, json_to_db_value};
use crate::model::Model;
use crate::orm::sql_builder::ArrayType;
use crate::orm::{ColumnTrait, ColumnType};

/// The ORM column type of `column` when it names one of `M`'s own columns,
/// given as the field or column name, optionally qualified with `M`'s table.
pub(crate) fn column_type_of<M: Model>(column: &str) -> Option<ColumnType> {
    let column = M::column_from_str(M::own_column_name(column)?)?;
    Some(column.def().get_column_type().clone())
}

/// [`column_type_of`] for the model registration a generated model submits.
#[doc(hidden)]
pub fn __column_type_of<M: Model>(column: &str) -> Option<ColumnType> {
    column_type_of::<M>(column)
}

/// Bind `value` for a column of `column_type`, or generically when the type is
/// unknown or the value does not parse as that type — the database then
/// reports the mismatch exactly as it did before.
pub(crate) fn json_to_column_value(
    value: &serde_json::Value,
    column_type: Option<&ColumnType>,
) -> Value {
    let Some(column_type) = column_type else {
        return json_to_db_value(value);
    };
    if value.is_null() {
        return typed_null(column_type);
    }
    typed_value(value, column_type).unwrap_or_else(|| json_to_db_value(value))
}

/// A JSON array as the PostgreSQL array a model field of `element`s binds,
/// each item read as a value of the element type at the element's width;
/// `None` when an item is not one.
fn array_value(value: &serde_json::Value, element: &ColumnType) -> Option<Value> {
    let array_type = array_type_for(element)?;
    let items = value
        .as_array()?
        .iter()
        .map(|item| {
            let item = match element_value(item, element) {
                Value::Double(Some(float))
                    if array_type == ArrayType::Float && (float as f32).is_finite() =>
                {
                    Value::from(float as f32)
                }
                item if crate::query::is_sql_null(&item) => {
                    narrow_null(&array_type).unwrap_or(item)
                }
                item => integer_as(item, array_type.clone()),
            };
            (item.array_type() == array_type).then_some(item)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Value::Array(array_type, Some(Box::new(items))))
}

/// The null of an integer or `f32` array's element, which `typed_null` reads
/// as the widest type; every other element's null is of its own type already.
fn narrow_null(array_type: &ArrayType) -> Option<Value> {
    Some(match array_type {
        ArrayType::TinyInt => Value::TinyInt(None),
        ArrayType::SmallInt => Value::SmallInt(None),
        ArrayType::Int => Value::Int(None),
        ArrayType::TinyUnsigned => Value::TinyUnsigned(None),
        ArrayType::SmallUnsigned => Value::SmallUnsigned(None),
        ArrayType::Unsigned => Value::Unsigned(None),
        ArrayType::BigUnsigned => Value::BigUnsigned(None),
        ArrayType::Float => Value::Float(None),
        _ => return None,
    })
}

/// The element type of the array a model field of `element`s binds.
fn array_type_for(element: &ColumnType) -> Option<ArrayType> {
    Some(match element {
        ColumnType::Boolean => ArrayType::Bool,
        ColumnType::TinyInteger => ArrayType::TinyInt,
        ColumnType::SmallInteger => ArrayType::SmallInt,
        ColumnType::Integer => ArrayType::Int,
        ColumnType::BigInteger => ArrayType::BigInt,
        ColumnType::TinyUnsigned => ArrayType::TinyUnsigned,
        ColumnType::SmallUnsigned => ArrayType::SmallUnsigned,
        ColumnType::Unsigned => ArrayType::Unsigned,
        ColumnType::BigUnsigned => ArrayType::BigUnsigned,
        ColumnType::Float => ArrayType::Float,
        ColumnType::Double => ArrayType::Double,
        ColumnType::Text | ColumnType::String(_) => ArrayType::String,
        ColumnType::Json | ColumnType::JsonBinary => ArrayType::Json,
        ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_) => ArrayType::Bytes,
        ColumnType::Uuid => ArrayType::Uuid,
        ColumnType::Decimal(_) | ColumnType::Money(_) => ArrayType::Decimal,
        ColumnType::Date => ArrayType::ChronoDate,
        ColumnType::Time => ArrayType::ChronoTime,
        ColumnType::DateTime | ColumnType::Timestamp => ArrayType::ChronoDateTime,
        ColumnType::TimestampWithTimeZone => ArrayType::ChronoDateTimeUtc,
        _ => return None,
    })
}

fn typed_value(value: &serde_json::Value, column_type: &ColumnType) -> Option<Value> {
    use serde_json::Value as Json;

    match (column_type, value) {
        (ColumnType::Array(element), Json::Array(_)) => array_value(value, element),
        (ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_), Json::Array(_)) => {
            Vec::<u8>::deserialize(value).ok().map(Value::from)
        }
        (ColumnType::Uuid, Json::String(text)) => uuid::Uuid::parse_str(text).ok().map(Value::from),
        (ColumnType::TimestampWithTimeZone, Json::String(text)) => {
            parse_utc_datetime(text).map(Value::from)
        }
        (ColumnType::DateTime | ColumnType::Timestamp, Json::String(text)) => {
            parse_naive_datetime(text).map(Value::from)
        }
        (ColumnType::Date, Json::String(text)) => parse_date(text).map(Value::from),
        (ColumnType::Time, Json::String(text)) => parse_time(text).map(Value::from),
        (ColumnType::Decimal(_) | ColumnType::Money(_), Json::String(text)) => {
            parse_decimal(text).map(Value::from)
        }
        (ColumnType::Decimal(_) | ColumnType::Money(_), Json::Number(number)) => {
            parse_decimal(&number.to_string()).map(Value::from)
        }
        (column_type, Json::String(text)) if is_integer(column_type) => {
            text.trim().parse::<i64>().ok().map(Value::from)
        }
        (ColumnType::Float | ColumnType::Double, Json::String(text)) => {
            text.trim().parse::<f64>().ok().map(Value::from)
        }
        (ColumnType::Float | ColumnType::Double, Json::Number(number)) => {
            number.as_f64().map(Value::from)
        }
        (ColumnType::String(_) | ColumnType::Text | ColumnType::Char(_), Json::Number(number)) => {
            Some(Value::from(number.to_string()))
        }
        _ => None,
    }
}

/// A NULL of the column's own type. A NULL bound as text is a type error
/// against a non-text column on PostgreSQL (`integer = text`), even where SQL
/// itself is happy to compare with NULL.
fn typed_null(column_type: &ColumnType) -> Value {
    match column_type {
        ColumnType::Json | ColumnType::JsonBinary => Value::Json(None),
        ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_) => Value::Bytes(None),
        ColumnType::Array(element) => array_type_for(element).map_or_else(
            || json_to_db_value(&serde_json::Value::Null),
            |array_type| Value::Array(array_type, None),
        ),
        ColumnType::Uuid => Value::from(None::<uuid::Uuid>),
        ColumnType::TimestampWithTimeZone => Value::from(None::<DateTime<Utc>>),
        ColumnType::DateTime | ColumnType::Timestamp => Value::from(None::<NaiveDateTime>),
        ColumnType::Date => Value::from(None::<NaiveDate>),
        ColumnType::Time => Value::from(None::<NaiveTime>),
        ColumnType::Decimal(_) | ColumnType::Money(_) => Value::from(None::<Decimal>),
        ColumnType::Boolean => Value::from(None::<bool>),
        ColumnType::Float | ColumnType::Double => Value::from(None::<f64>),
        column_type if is_integer(column_type) => Value::from(None::<i64>),
        _ => json_to_db_value(&serde_json::Value::Null),
    }
}

/// Whether `value` is a non-NULL integer, signed or not.
pub(crate) fn is_integer_value(value: &Value) -> bool {
    matches!(
        value,
        Value::TinyInt(Some(_))
            | Value::SmallInt(Some(_))
            | Value::Int(Some(_))
            | Value::BigInt(Some(_))
            | Value::TinyUnsigned(Some(_))
            | Value::SmallUnsigned(Some(_))
            | Value::Unsigned(Some(_))
            | Value::BigUnsigned(Some(_))
    )
}

/// Whether `column_type` holds integers, signed or not.
pub(crate) fn is_integer(column_type: &ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::TinyInteger
            | ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::TinyUnsigned
            | ColumnType::SmallUnsigned
            | ColumnType::Unsigned
            | ColumnType::BigUnsigned
    )
}

/// RFC 3339 (`T` or space separator, any offset), a naive date-time taken as
/// UTC, or a bare date at midnight UTC.
fn parse_utc_datetime(text: &str) -> Option<DateTime<Utc>> {
    DateTime::<FixedOffset>::from_str(text)
        .map(|moment| moment.with_timezone(&Utc))
        .ok()
        .or_else(|| parse_naive_datetime(text).map(|naive| naive.and_utc()))
}

/// `YYYY-MM-DD[T ]HH:MM:SS[.fraction]`, an offset-carrying timestamp reduced
/// to UTC, or a bare date at midnight.
fn parse_naive_datetime(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::from_str(text)
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f").ok())
        .or_else(|| {
            DateTime::<FixedOffset>::from_str(text)
                .ok()
                .map(|moment| moment.naive_utc())
        })
        .or_else(|| parse_date(text).and_then(|date| date.and_hms_opt(0, 0, 0)))
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::from_str(text).ok()
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    NaiveTime::from_str(text)
        .ok()
        .or_else(|| NaiveTime::parse_from_str(text, "%H:%M").ok())
}

fn parse_decimal(text: &str) -> Option<Decimal> {
    Decimal::from_str_exact(text)
        .or_else(|_| Decimal::from_scientific(text))
        .ok()
}

#[cfg(test)]
#[path = "../../tests/unit/internal_column_values_tests.rs"]
mod tests;

/// Bind an element using the declared SQL array element type.
fn json_to_array_element(value: &serde_json::Value, column: Option<&ColumnType>) -> Value {
    match column {
        Some(ColumnType::Array(element)) => element_value(value, element),
        _ => json_to_db_value(value),
    }
}

/// `item` as an element of an array of `element`s: a JSON document for an
/// array of JSON, and otherwise a value of the element type.
fn element_value(item: &serde_json::Value, element: &ColumnType) -> Value {
    // Every serde_json::Value, including Null, is a JSON document inside a
    // JSON array. SQL NULL would corrupt a Vec<Value> on its next read.
    if matches!(element, ColumnType::Json | ColumnType::JsonBinary) {
        return Value::from(item.clone());
    }
    json_to_column_value(item, Some(element))
}

/// Bind the filter value `value` for a column of `column_type`: as itself when
/// the column holds its type or the type is unknown, and otherwise, like a
/// JSON value, read as the column's type by [`json_to_column_value`].
pub(crate) fn column_value(value: &Value, column_type: Option<&ColumnType>) -> Value {
    match (value, column_type) {
        (Value::Json(Some(json)), _) => json_to_column_value(json, column_type),
        (_, Some(column_type)) if !holds(column_type, value) => {
            json_to_column_value(&value_document(value), Some(column_type))
        }
        _ => super::bindable_value(value.clone()),
    }
}

/// Bind `value` as an element of the array column `column` holds: a JSON
/// document for an array of JSON, and otherwise a value of the element type.
pub(crate) fn array_element(value: &Value, column: Option<&ColumnType>) -> Value {
    if let Value::Json(Some(json)) = value {
        return json_to_array_element(json, column);
    }
    if let Some(ColumnType::Array(element)) = column {
        return match element.as_ref() {
            ColumnType::Json | ColumnType::JsonBinary => Value::from(value_document(value)),
            element => column_value(value, Some(element)),
        };
    }
    super::bindable_value(value.clone())
}

/// Whether a column of `column_type` holds values of `value`'s type, so that
/// binding it unchanged compares what a model field of that type writes. A
/// type this does not know binds unchanged.
fn holds(column_type: &ColumnType, value: &Value) -> bool {
    match column_type {
        column_type if is_integer(column_type) => matches!(
            value,
            Value::TinyInt(_)
                | Value::SmallInt(_)
                | Value::Int(_)
                | Value::BigInt(_)
                | Value::TinyUnsigned(_)
                | Value::SmallUnsigned(_)
                | Value::Unsigned(_)
                | Value::BigUnsigned(_)
        ),
        ColumnType::Float | ColumnType::Double => {
            matches!(value, Value::Float(_) | Value::Double(_))
        }
        ColumnType::Decimal(_) | ColumnType::Money(_) => matches!(value, Value::Decimal(_)),
        ColumnType::String(_) | ColumnType::Text | ColumnType::Char(_) => {
            matches!(value, Value::String(_) | Value::Char(_))
        }
        ColumnType::Boolean => matches!(value, Value::Bool(_)),
        ColumnType::Uuid => matches!(value, Value::Uuid(_)),
        ColumnType::TimestampWithTimeZone => matches!(
            value,
            Value::ChronoDateTimeUtc(_)
                | Value::ChronoDateTimeLocal(_)
                | Value::ChronoDateTimeWithTimeZone(_)
        ),
        ColumnType::DateTime | ColumnType::Timestamp => matches!(value, Value::ChronoDateTime(_)),
        ColumnType::Date => matches!(value, Value::ChronoDate(_)),
        ColumnType::Time => matches!(value, Value::ChronoTime(_)),
        ColumnType::Json | ColumnType::JsonBinary => matches!(value, Value::Json(_)),
        ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_) => {
            matches!(value, Value::Bytes(_))
        }
        ColumnType::Array(element) => match (value, array_type_for(element)) {
            (Value::Array(held, _), Some(element)) => *held == element,
            (value, _) => matches!(value, Value::Array(..)),
        },
        _ => true,
    }
}

/// `value` as the integer of type `field` when both are integers and it
/// fits, so a key read from one model's field can be set on another's of
/// another width; anything else as it is.
pub(crate) fn integer_as(value: Value, field: ArrayType) -> Value {
    let integer = match value {
        Value::TinyInt(Some(n)) => i128::from(n),
        Value::SmallInt(Some(n)) => i128::from(n),
        Value::Int(Some(n)) => i128::from(n),
        Value::BigInt(Some(n)) => i128::from(n),
        Value::TinyUnsigned(Some(n)) => i128::from(n),
        Value::SmallUnsigned(Some(n)) => i128::from(n),
        Value::Unsigned(Some(n)) => i128::from(n),
        Value::BigUnsigned(Some(n)) => i128::from(n),
        _ => return value,
    };
    let converted = match field {
        ArrayType::TinyInt => i8::try_from(integer).ok().map(Value::from),
        ArrayType::SmallInt => i16::try_from(integer).ok().map(Value::from),
        ArrayType::Int => i32::try_from(integer).ok().map(Value::from),
        ArrayType::BigInt => i64::try_from(integer).ok().map(Value::from),
        ArrayType::TinyUnsigned => u8::try_from(integer).ok().map(Value::from),
        ArrayType::SmallUnsigned => u16::try_from(integer).ok().map(Value::from),
        ArrayType::Unsigned => u32::try_from(integer).ok().map(Value::from),
        ArrayType::BigUnsigned => u64::try_from(integer).ok().map(Value::from),
        _ => None,
    };
    converted.unwrap_or(value)
}

/// `value` as a JSON document: a JSON value as itself, SQL NULL as `null`,
/// and anything else as the JSON its Rust type serializes to.
pub(crate) fn value_document(value: &Value) -> serde_json::Value {
    use serde_json::to_value;

    if *value == value.as_null() {
        return serde_json::Value::Null;
    }
    let document = match value {
        Value::Json(Some(json)) => return json.as_ref().clone(),
        Value::ChronoDate(Some(date)) => to_value(date),
        Value::ChronoTime(Some(time)) => to_value(time),
        Value::ChronoDateTime(Some(moment)) => to_value(moment),
        Value::ChronoDateTimeUtc(Some(moment)) => to_value(moment),
        Value::ChronoDateTimeLocal(Some(moment)) => to_value(moment),
        Value::ChronoDateTimeWithTimeZone(Some(moment)) => to_value(moment),
        Value::Decimal(Some(decimal)) => to_value(decimal),
        Value::Bytes(Some(bytes)) => to_value(bytes),
        Value::Array(_, Some(items)) => {
            return serde_json::Value::Array(items.iter().map(value_document).collect());
        }
        other => {
            return crate::orm::sql_builder::value_to_json_value(other)
                .unwrap_or_else(|_| serde_json::Value::String(other.to_string()));
        }
    };
    document.unwrap_or(serde_json::Value::Null)
}
