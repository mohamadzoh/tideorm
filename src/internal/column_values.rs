//! Bind filter and assignment values with the type of the column they target.
//!
//! Filter values travel as JSON, where a UUID, a timestamp or a decimal is a
//! string. Bound as text, that string is rejected by PostgreSQL (`uuid = text`)
//! and silently matches nothing on SQLite, which stores UUIDs as 16-byte blobs,
//! and on MySQL's `BINARY(16)`. Knowing the model's column type turns the value
//! back into the database type the column holds before it is bound.

use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;

use super::{Value, json_to_db_value};
use crate::model::Model;
use crate::orm::{ColumnTrait, ColumnType, EntityTrait, IdenStatic, Iterable};

/// The ORM column type of `column` when it names one of `M`'s own columns,
/// given as the field or column name, optionally qualified with `M`'s table.
pub(crate) fn column_type_of<M: Model>(column: &str) -> Option<ColumnType> {
    let name = match column.split_once('.') {
        Some((table, name)) if table == M::table_name() => name,
        Some(_) => return None,
        None => column,
    };
    let name = M::canonical_column_name(name)?;
    <<M as crate::internal::InternalModel>::Entity as EntityTrait>::Column::iter()
        .find(|candidate| candidate.as_str() == name)
        .map(|candidate| candidate.def().get_column_type().clone())
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

fn typed_value(value: &serde_json::Value, column_type: &ColumnType) -> Option<Value> {
    use serde_json::Value as Json;

    match (column_type, value) {
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

fn is_integer(column_type: &ColumnType) -> bool {
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
