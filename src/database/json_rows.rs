//! Decoding result rows into JSON objects for `get_json()` and the raw JSON
//! entry points.
//!
//! A column is decoded by the type the database declares for it, never by what
//! its value looks like: a text column holding `00123`, `1e3` or `null` comes
//! back as that string. Guessing is what turned zip codes into numbers.
//!
//! A declared type can still lose information. SQLite keeps booleans as
//! integers and JSON, dates and UUIDs as text, and MySQL has no zoned
//! timestamp, so when rows come from a model query the model's own column type
//! is tried first. A model column then comes back as the model's JSON has it,
//! on every backend.

use crate::internal::{QueryResult, TryGetable};
use crate::orm::ColumnType;

use super::Database;

/// The model column type of a result column, for rows read by a model query.
pub(crate) type ColumnTypeLookup<'a> = &'a (dyn Fn(&str) -> Option<ColumnType> + Sync);

/// How one result column is decoded. Every row of a result carries the same
/// columns, so this is worked out once per result rather than per row.
struct ColumnPlan {
    name: String,
    /// The model's type for the column, tried first.
    model_type: Option<ColumnType>,
    /// The type the database declares, upper-cased, for `decoder`.
    declared_type: String,
    decoder: fn(&QueryResult, usize, &str) -> serde_json::Value,
}

impl Database {
    /// Decode `rows` into JSON objects, each column by its model type when
    /// `model_type` knows it and by its declared type otherwise.
    pub(super) fn query_rows_to_json(
        rows: &[QueryResult],
        model_type: ColumnTypeLookup<'_>,
    ) -> Vec<serde_json::Value> {
        let Some(first) = rows.first() else {
            return Vec::new();
        };
        let plan = Self::column_plan(first, model_type);
        rows.iter()
            .map(|row| {
                let mut object = serde_json::Map::with_capacity(plan.len());
                for (index, column) in plan.iter().enumerate() {
                    let value = column
                        .model_type
                        .as_ref()
                        .and_then(|column_type| Self::model_column_to_json(row, index, column_type))
                        .unwrap_or_else(|| (column.decoder)(row, index, &column.declared_type));
                    object.insert(column.name.clone(), value);
                }
                serde_json::Value::Object(object)
            })
            .collect()
    }

    fn column_plan(row: &QueryResult, model_type: ColumnTypeLookup<'_>) -> Vec<ColumnPlan> {
        #[cfg(feature = "postgres")]
        if let Some(pg_row) = row.try_as_pg_row() {
            return Self::sqlx_column_plan(pg_row, model_type, Self::postgres_column_to_json);
        }

        #[cfg(feature = "mysql")]
        if let Some(mysql_row) = row.try_as_mysql_row() {
            return Self::sqlx_column_plan(mysql_row, model_type, Self::mysql_column_to_json);
        }

        #[cfg(feature = "sqlite")]
        if let Some(sqlite_row) = row.try_as_sqlite_row() {
            return Self::sqlx_column_plan(sqlite_row, model_type, Self::sqlite_column_to_json);
        }

        row.column_names()
            .into_iter()
            .map(|name| ColumnPlan {
                model_type: model_type(&name),
                name,
                declared_type: String::new(),
                decoder: |row, index, _| Self::fallback_try_get_json(row, index),
            })
            .collect()
    }

    #[cfg(feature = "postgres")]
    fn postgres_column_to_json(
        row: &QueryResult,
        index: usize,
        type_name: &str,
    ) -> serde_json::Value {
        match type_name {
            "BOOL" => Self::typed_or_fallback::<bool>(row, index),
            "INT2" => Self::typed_or_fallback::<i16>(row, index),
            "INT4" => Self::typed_or_fallback::<i32>(row, index),
            "INT8" => Self::typed_or_fallback::<i64>(row, index),
            "FLOAT4" => Self::float_or_fallback::<f32>(row, index),
            "FLOAT8" => Self::float_or_fallback::<f64>(row, index),
            "NUMERIC" => Self::decimal_or_fallback(row, index),
            "UUID" => Self::typed_or_fallback::<uuid::Uuid>(row, index),
            "JSON" | "JSONB" => Self::typed_or_fallback::<serde_json::Value>(row, index),
            "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(row, index),
            "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(row, index),
            "TIMESTAMP" => Self::typed_or_fallback::<chrono::NaiveDateTime>(row, index),
            "TIMESTAMPTZ" => {
                Self::typed_or_fallback::<chrono::DateTime<chrono::FixedOffset>>(row, index)
            }
            "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "CITEXT" | "UNKNOWN" => {
                Self::typed_or_fallback::<String>(row, index)
            }
            "BYTEA" => Self::typed_or_fallback::<Vec<u8>>(row, index),
            "BOOL[]" => Self::typed_or_fallback::<Vec<bool>>(row, index),
            "INT2[]" => Self::typed_or_fallback::<Vec<i16>>(row, index),
            "INT4[]" => Self::typed_or_fallback::<Vec<i32>>(row, index),
            "INT8[]" => Self::typed_or_fallback::<Vec<i64>>(row, index),
            "FLOAT4[]" => Self::typed_or_fallback::<Vec<f32>>(row, index),
            "FLOAT8[]" => Self::typed_or_fallback::<Vec<f64>>(row, index),
            "NUMERIC[]" => Self::typed_or_fallback::<Vec<rust_decimal::Decimal>>(row, index),
            "UUID[]" => Self::typed_or_fallback::<Vec<uuid::Uuid>>(row, index),
            "JSON[]" | "JSONB[]" => Self::typed_or_fallback::<Vec<serde_json::Value>>(row, index),
            "TEXT[]" | "VARCHAR[]" | "CHAR[]" | "NAME[]" => {
                Self::typed_or_fallback::<Vec<String>>(row, index)
            }
            _ => Self::fallback_try_get_json(row, index),
        }
    }

    #[cfg(feature = "mysql")]
    fn mysql_column_to_json(row: &QueryResult, index: usize, type_name: &str) -> serde_json::Value {
        match type_name {
            "BOOLEAN" | "BOOL" => Self::typed_or_fallback::<bool>(row, index),
            "TINYINT" => Self::typed_or_fallback::<i8>(row, index),
            "SMALLINT" => Self::typed_or_fallback::<i16>(row, index),
            "INT" | "INTEGER" | "MEDIUMINT" => Self::typed_or_fallback::<i32>(row, index),
            "BIGINT" => Self::typed_or_fallback::<i64>(row, index),
            // Window functions such as ROW_NUMBER() and RANK() are unsigned.
            "TINYINT UNSIGNED" => Self::typed_or_fallback::<u8>(row, index),
            "SMALLINT UNSIGNED" => Self::typed_or_fallback::<u16>(row, index),
            "INT UNSIGNED" | "MEDIUMINT UNSIGNED" => Self::typed_or_fallback::<u32>(row, index),
            "BIGINT UNSIGNED" => Self::typed_or_fallback::<u64>(row, index),
            "FLOAT" => Self::float_or_fallback::<f32>(row, index),
            "DOUBLE" => Self::float_or_fallback::<f64>(row, index),
            "DECIMAL" | "NUMERIC" => Self::decimal_or_fallback(row, index),
            "JSON" => Self::typed_or_fallback::<serde_json::Value>(row, index),
            "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(row, index),
            "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(row, index),
            "DATETIME" | "TIMESTAMP" => {
                Self::typed_or_fallback::<chrono::NaiveDateTime>(row, index)
            }
            "CHAR" | "VARCHAR" | "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM"
            | "SET" => Self::typed_or_fallback::<String>(row, index),
            // Text with a binary collation (`utf8mb4_bin`, which MariaDB gives
            // every JSON column) is named like binary data. The driver decodes
            // a string only from a column whose collation is not `binary`, so
            // trying one first tells the two apart by declaration.
            // A UUID column is `BINARY(16)`.
            "BINARY" => Self::try_get_json::<String>(row, index)
                .or_else(|| Self::try_get_json::<uuid::Uuid>(row, index))
                .unwrap_or_else(|| Self::typed_or_fallback::<Vec<u8>>(row, index)),
            "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
                Self::try_get_json::<String>(row, index)
                    .unwrap_or_else(|| Self::typed_or_fallback::<Vec<u8>>(row, index))
            }
            _ => Self::fallback_try_get_json(row, index),
        }
    }

    #[cfg(feature = "sqlite")]
    fn sqlite_column_to_json(
        row: &QueryResult,
        index: usize,
        type_name: &str,
    ) -> serde_json::Value {
        match type_name {
            "BOOLEAN" | "BOOL" => Self::typed_or_fallback::<bool>(row, index),
            "INTEGER" | "INT" => Self::typed_or_fallback::<i64>(row, index),
            "REAL" | "FLOAT" | "DOUBLE" => Self::float_or_fallback::<f64>(row, index),
            "NUMERIC" | "DECIMAL" => Self::decimal_or_fallback(row, index),
            "JSON" => Self::typed_or_fallback::<serde_json::Value>(row, index),
            "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(row, index),
            "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(row, index),
            "DATETIME" | "TIMESTAMP" => {
                Self::typed_or_fallback::<chrono::NaiveDateTime>(row, index)
            }
            "TEXT" => Self::typed_or_fallback::<String>(row, index),
            "BLOB" => Self::typed_or_fallback::<Vec<u8>>(row, index),
            _ => Self::sqlite_value_to_json(row, index),
        }
    }

    /// Decode a SQLite column the driver knows no declared type for — an
    /// expression, or a column declared as something like `JSON` or `DECIMAL`
    /// — by the storage class of the value itself.
    ///
    /// Text comes back verbatim unless it holds a JSON object or array, which
    /// is how SQLite's JSON functions and `JSON` columns hand documents over.
    /// A real, as from `SUM()` over a `REAL` column or from a `DECIMAL`
    /// column, is the number SQLite stores: the driver cannot tell the two
    /// apart, and a decimal would turn a float sum into a string. A model
    /// query still reads its own `Decimal` fields as decimals.
    #[cfg(feature = "sqlite")]
    fn sqlite_value_to_json(row: &QueryResult, index: usize) -> serde_json::Value {
        use crate::internal::sqlx::{Row, TypeInfo, ValueRef};

        let storage_class = row
            .try_as_sqlite_row()
            .and_then(|sqlite_row| sqlite_row.try_get_raw(index).ok())
            .map(|value| value.type_info().name().to_string());
        match storage_class.as_deref() {
            Some("NULL") => serde_json::Value::Null,
            Some("INTEGER") => Self::typed_or_fallback::<i64>(row, index),
            Some("REAL") => Self::float_or_fallback::<f64>(row, index),
            Some("TEXT") => match row.try_get_by_index::<Option<String>>(index) {
                Ok(Some(text)) => match serde_json::from_str(&text) {
                    Ok(document @ (serde_json::Value::Object(_) | serde_json::Value::Array(_))) => {
                        document
                    }
                    _ => serde_json::Value::String(text),
                },
                _ => Self::fallback_try_get_json(row, index),
            },
            _ => Self::fallback_try_get_json(row, index),
        }
    }

    /// Decode a column as the Rust type a model reads `column_type` into, so
    /// the value matches that model's JSON. `None` when the column type is not
    /// a plain scalar or the stored value does not decode as it — a select
    /// alias can reuse a model column's name for something else entirely.
    fn model_column_to_json(
        row: &QueryResult,
        index: usize,
        column_type: &ColumnType,
    ) -> Option<serde_json::Value> {
        match column_type {
            ColumnType::Boolean => Self::try_get_json::<bool>(row, index),
            ColumnType::TinyInteger => Self::try_get_json::<i8>(row, index),
            ColumnType::SmallInteger => Self::try_get_json::<i16>(row, index),
            ColumnType::Integer => Self::try_get_json::<i32>(row, index),
            ColumnType::BigInteger => Self::try_get_json::<i64>(row, index),
            ColumnType::TinyUnsigned => Self::try_get_json::<u8>(row, index),
            ColumnType::SmallUnsigned => Self::try_get_json::<u16>(row, index),
            ColumnType::Unsigned => Self::try_get_json::<u32>(row, index),
            ColumnType::BigUnsigned => Self::try_get_json::<u64>(row, index),
            ColumnType::Float => Self::try_get_float_json::<f32>(row, index),
            ColumnType::Double => Self::try_get_float_json::<f64>(row, index),
            ColumnType::Decimal(_) | ColumnType::Money(_) => {
                Self::try_get_json::<rust_decimal::Decimal>(row, index)
            }
            ColumnType::Char(_) | ColumnType::String(_) | ColumnType::Text => {
                Self::try_get_json::<String>(row, index)
            }
            ColumnType::Uuid => Self::try_get_json::<uuid::Uuid>(row, index),
            ColumnType::Json | ColumnType::JsonBinary => {
                Self::try_get_json::<serde_json::Value>(row, index)
            }
            ColumnType::Date => Self::try_get_json::<chrono::NaiveDate>(row, index),
            ColumnType::Time => Self::try_get_json::<chrono::NaiveTime>(row, index),
            ColumnType::DateTime | ColumnType::Timestamp => {
                Self::try_get_json::<chrono::NaiveDateTime>(row, index)
            }
            ColumnType::TimestampWithTimeZone => {
                Self::try_get_json::<chrono::DateTime<chrono::Utc>>(row, index)
            }
            ColumnType::Binary(_) | ColumnType::VarBinary(_) | ColumnType::Blob => {
                Self::try_get_json::<Vec<u8>>(row, index)
            }
            _ => None,
        }
    }

    /// Decode a column whose declared type named nothing more specific.
    ///
    /// Only typed decoders are tried: a text value is never re-read as a
    /// number, so whatever reaches the `String` step comes back verbatim.
    fn fallback_try_get_json(row: &QueryResult, index: usize) -> serde_json::Value {
        Self::try_get_json::<serde_json::Value>(row, index)
            .or_else(|| Self::try_get_json::<uuid::Uuid>(row, index))
            .or_else(|| Self::try_get_json::<rust_decimal::Decimal>(row, index))
            .or_else(|| Self::try_get_json::<i64>(row, index))
            .or_else(|| Self::try_get_json::<u64>(row, index))
            .or_else(|| Self::try_get_json::<f64>(row, index))
            .or_else(|| Self::try_get_json::<bool>(row, index))
            // SQLite decodes a date from an integer too, so dates come after
            // the numbers.
            .or_else(|| Self::try_get_json::<chrono::DateTime<chrono::FixedOffset>>(row, index))
            .or_else(|| Self::try_get_json::<chrono::DateTime<chrono::Utc>>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveDateTime>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveDate>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveTime>(row, index))
            .or_else(|| Self::try_get_json::<String>(row, index))
            .or_else(|| Self::try_get_json::<Vec<u8>>(row, index))
            .or_else(|| Self::undecoded_as_text(row, index))
            .unwrap_or(serde_json::Value::Null)
    }

    /// A value no typed decoder accepts, as the text the server sent for it,
    /// so a non-NULL value (a PostgreSQL enum label, a MySQL `DECIMAL` past
    /// `Decimal`'s 28 digits) does not come back as `null`, which reads as
    /// SQL `NULL`. `None` for a NULL, and for a binary value that is not
    /// printable text.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql")),
        allow(unused_variables)
    )]
    fn undecoded_as_text(row: &QueryResult, index: usize) -> Option<serde_json::Value> {
        #[cfg(feature = "postgres")]
        if let Some(pg_row) = row.try_as_pg_row() {
            use crate::internal::sqlx::postgres::PgValueFormat;
            use crate::internal::sqlx::{Row, ValueRef};

            let raw = pg_row.try_get_raw(index).ok()?;
            if raw.is_null() {
                return None;
            }
            let text = match raw.format() {
                PgValueFormat::Text => raw.as_str().ok()?,
                PgValueFormat::Binary => std::str::from_utf8(raw.as_bytes().ok()?).ok()?,
            };
            let printable = text
                .chars()
                .all(|character| !character.is_control() || character.is_whitespace());
            return printable.then(|| serde_json::Value::String(text.to_string()));
        }

        #[cfg(feature = "mysql")]
        if let Some(mysql_row) = row.try_as_mysql_row() {
            use crate::internal::sqlx::Row;

            // MySQL sends a DECIMAL as its digits, in either protocol.
            return mysql_row
                .try_get_unchecked::<Option<String>, _>(index)
                .ok()
                .flatten()
                .map(serde_json::Value::String);
        }

        None
    }

    /// A PostgreSQL `NUMERIC` in its binary form as exact decimal text:
    /// `Decimal` holds 28 digits, and a `NUMERIC` holds far more, or `NaN`.
    #[cfg(feature = "postgres")]
    fn postgres_numeric_as_text(row: &QueryResult, index: usize) -> Option<serde_json::Value> {
        use crate::internal::sqlx::postgres::PgValueFormat;
        use crate::internal::sqlx::{Row, ValueRef};

        let raw = row.try_as_pg_row()?.try_get_raw(index).ok()?;
        if raw.is_null() || raw.format() != PgValueFormat::Binary {
            return None;
        }
        numeric_text(raw.as_bytes().ok()?).map(serde_json::Value::String)
    }

    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn typed_or_fallback<T>(row: &QueryResult, index: usize) -> serde_json::Value
    where
        T: TryGetable + serde::Serialize,
    {
        Self::try_get_json::<T>(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    /// Decode a floating-point column, keeping non-finite values visible.
    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn float_or_fallback<T>(row: &QueryResult, index: usize) -> serde_json::Value
    where
        T: TryGetable + Into<f64>,
    {
        Self::try_get_float_json::<T>(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    fn try_get_float_json<T>(row: &QueryResult, index: usize) -> Option<serde_json::Value>
    where
        T: TryGetable + Into<f64>,
    {
        match row.try_get_by_index::<Option<T>>(index) {
            Ok(Some(value)) => Some(Self::f64_to_json(value.into())),
            Ok(None) => Some(serde_json::Value::Null),
            Err(_) => None,
        }
    }

    /// Represent an `f64` as JSON.
    ///
    /// JSON has no `NaN` or `Infinity`, so those are rendered as strings — a
    /// `null` would be indistinguishable from a real SQL `NULL`.
    pub(super) fn f64_to_json(value: f64) -> serde_json::Value {
        match serde_json::Number::from_f64(value) {
            Some(number) => serde_json::Value::Number(number),
            None => serde_json::Value::String(value.to_string()),
        }
    }

    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn decimal_or_fallback(row: &QueryResult, index: usize) -> serde_json::Value {
        Self::try_get_decimal_json(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    /// Decode a column declared as an exact numeric, whichever Rust type the
    /// driver hands it over as.
    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn try_get_decimal_json(row: &QueryResult, index: usize) -> Option<serde_json::Value> {
        if let Some(value) = Self::try_get_json::<rust_decimal::Decimal>(row, index) {
            return Some(value);
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<String>>(index) {
            return rust_decimal::Decimal::from_str_exact(&value)
                .ok()
                .and_then(|decimal| serde_json::to_value(decimal).ok())
                .or(Some(serde_json::Value::String(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<i64>>(index) {
            return serde_json::to_value(rust_decimal::Decimal::from(value))
                .ok()
                .or(Some(serde_json::json!(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<u64>>(index) {
            return serde_json::to_value(rust_decimal::Decimal::from(value))
                .ok()
                .or(Some(serde_json::json!(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<f64>>(index) {
            let value_text = value.to_string();
            return Some(
                rust_decimal::Decimal::from_str_exact(&value_text)
                    .ok()
                    .and_then(|decimal| serde_json::to_value(decimal).ok())
                    .unwrap_or_else(|| Self::f64_to_json(value)),
            );
        }

        #[cfg(feature = "postgres")]
        if let Some(value) = Self::postgres_numeric_as_text(row, index) {
            return Some(value);
        }

        None
    }

    fn try_get_json<T>(row: &QueryResult, index: usize) -> Option<serde_json::Value>
    where
        T: TryGetable + serde::Serialize,
    {
        row.try_get_by_index::<Option<T>>(index)
            .ok()
            .and_then(Self::option_to_json)
    }

    /// Convert a decoded column into JSON.
    ///
    /// Returns `None` when the value decoded but could not be represented as
    /// JSON, so callers keep trying other decoders instead of reporting a
    /// value that never existed. Only a real SQL `NULL` yields
    /// `Some(Value::Null)`.
    fn option_to_json<T>(value: Option<T>) -> Option<serde_json::Value>
    where
        T: serde::Serialize,
    {
        match value {
            Some(value) => serde_json::to_value(value).ok(),
            None => Some(serde_json::Value::Null),
        }
    }

    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn sqlx_column_plan<R>(
        row: &R,
        model_type: ColumnTypeLookup<'_>,
        decoder: fn(&QueryResult, usize, &str) -> serde_json::Value,
    ) -> Vec<ColumnPlan>
    where
        R: crate::internal::sqlx::Row,
    {
        use crate::internal::sqlx::{Column, TypeInfo};

        row.columns()
            .iter()
            .map(|column| ColumnPlan {
                name: column.name().to_string(),
                model_type: model_type(column.name()),
                declared_type: column.type_info().name().to_ascii_uppercase(),
                decoder,
            })
            .collect()
    }
}

/// The decimal text of a PostgreSQL `NUMERIC` in its binary wire form: a
/// digit count, the weight of the first base-10000 digit, the sign, the
/// display scale, then the digits.
#[cfg(feature = "postgres")]
fn numeric_text(bytes: &[u8]) -> Option<String> {
    let word = |at: usize| -> Option<u16> {
        Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
    };
    let ndigits = usize::from(word(0)?);
    let weight = i32::from(word(2)? as i16);
    let sign = word(4)?;
    let scale = usize::from(word(6)?);
    match sign {
        0xC000 => return Some("NaN".to_string()),
        0xD000 => return Some("Infinity".to_string()),
        0xF000 => return Some("-Infinity".to_string()),
        0x0000 | 0x4000 => {}
        _ => return None,
    }
    let digits = (0..ndigits)
        .map(|position| word(8 + 2 * position))
        .collect::<Option<Vec<_>>>()?;
    // The digit of base-10000 place `weight - group`; zero past the stored ones.
    let digit = |group: i32| {
        usize::try_from(group)
            .ok()
            .and_then(|group| digits.get(group).copied())
            .unwrap_or(0)
    };

    let mut text = String::new();
    if sign == 0x4000 {
        text.push('-');
    }
    if weight < 0 {
        text.push('0');
    } else {
        text.push_str(&digit(0).to_string());
        for group in 1..=weight {
            text.push_str(&format!("{:04}", digit(group)));
        }
    }
    if scale > 0 {
        let mut fraction = String::with_capacity(scale + 4);
        let mut group = weight + 1;
        while fraction.len() < scale {
            fraction.push_str(&format!("{:04}", digit(group)));
            group += 1;
        }
        fraction.truncate(scale);
        text.push('.');
        text.push_str(&fraction);
    }
    Some(text)
}

#[cfg(all(test, feature = "postgres"))]
#[path = "../../tests/unit/database_json_rows_tests.rs"]
mod numeric_text_tests;
