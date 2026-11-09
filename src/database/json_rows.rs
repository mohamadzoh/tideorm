//! Decoding result rows into JSON objects for `get_json()` and the raw JSON
//! entry points.
//!
//! A column is decoded by the type the database declares for it, never by what
//! its value looks like: a text column holding `00123`, `1e3` or `null` comes
//! back as that string. Guessing is what turned zip codes into numbers. Each
//! driver decodes a value by its declared type without narrowing it
//! (`decode_lossless`), and that value is mapped to JSON here.
//!
//! A declared type can still lose information. SQLite keeps booleans as
//! integers and JSON, dates and UUIDs as text, and MySQL has no zoned
//! timestamp, so when rows come from a model query the model's own column type
//! is tried first. A model column then comes back as the model's JSON has it,
//! on every backend.
//!
//! A value is never `null` unless it is SQL `NULL`: one no decoder can read
//! fails the read instead.

use crate::error::{Error, Result};
use crate::internal::{QueryResult, TryGetable};
use crate::orm::ColumnType;

use super::Database;

#[cfg(feature = "mysql")]
mod mysql;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(feature = "sqlite")]
mod sqlite_outputs;
#[cfg(feature = "sqlite")]
mod sqlite_schema;

/// The model column type of a result column, for rows read by a model query.
pub(crate) type ColumnTypeLookup<'a> = &'a (dyn Fn(&str) -> Option<ColumnType> + Sync);

/// What the driver cannot say about a SQLite result: which of its columns
/// hold JSON documents, a table column declared `JSON` or `JSONB` or an
/// expression a JSON function computed, in the statement or in a view it
/// reads.
#[derive(Default)]
pub(super) struct SqliteSchema {
    /// By result column; empty on other backends.
    documents: Vec<bool>,
}

/// How one result column is decoded. Every row of a result carries the same
/// columns, so this is worked out once per result rather than per row.
struct ColumnPlan {
    name: String,
    /// The model's type for the column, tried first.
    model_type: Option<ColumnType>,
    /// Whether the column holds JSON documents, whose text is read as one.
    document: bool,
}

impl Database {
    /// Decode `rows` into JSON objects, each column by its model type when
    /// `model_type` knows it and by its declared type otherwise.
    pub(super) fn query_rows_to_json(
        rows: &[QueryResult],
        model_type: ColumnTypeLookup<'_>,
        schema: &SqliteSchema,
    ) -> Result<Vec<serde_json::Value>> {
        let Some(first) = rows.first() else {
            return Ok(Vec::new());
        };
        let plan: Vec<ColumnPlan> = first
            .column_names()
            .into_iter()
            .enumerate()
            .map(|(index, name)| ColumnPlan {
                model_type: model_type(&name),
                document: schema.documents.get(index).copied().unwrap_or(false),
                name,
            })
            .collect();
        rows.iter()
            .map(|row| {
                let mut object = serde_json::Map::with_capacity(plan.len());
                for (index, column) in plan.iter().enumerate() {
                    let model_value = column.model_type.as_ref().and_then(|column_type| {
                        Self::model_column_to_json(row, index, column_type)
                    });
                    let value = match model_value {
                        Some(value) => value,
                        None => Self::decoded_json(row, index, column.document)
                            .map_err(|declared| Self::unreadable(&column.name, &declared))?,
                    };
                    object.insert(column.name.clone(), value);
                }
                Ok(serde_json::Value::Object(object))
            })
            .collect()
    }

    fn unreadable(column: &str, declared: &str) -> Error {
        let declared = match declared {
            "" => String::new(),
            declared => format!(" of type {declared}"),
        };
        Error::conversion(format!(
            "column '{column}'{declared} holds a value TideORM cannot read as JSON"
        ))
    }

    /// The [`SqliteSchema`] of `rows`: nothing to read without SQLite.
    #[cfg(not(feature = "sqlite"))]
    pub(super) async fn sqlite_schema(
        &self,
        _rows: &[QueryResult],
        _sql: &str,
        _model_type: ColumnTypeLookup<'_>,
    ) -> Result<SqliteSchema> {
        Ok(SqliteSchema::default())
    }

    /// The value the driver decodes by the column's declared type, as JSON;
    /// the declared type's name when it cannot be read.
    #[cfg_attr(not(feature = "sqlite"), allow(unused_variables))]
    fn decoded_json(
        row: &QueryResult,
        index: usize,
        document: bool,
    ) -> std::result::Result<serde_json::Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(row) = row.try_as_pg_row() {
            return postgres::cell_json(row, index);
        }
        #[cfg(feature = "mysql")]
        if let Some(row) = row.try_as_mysql_row() {
            return mysql::cell_json(row, index);
        }
        #[cfg(feature = "sqlite")]
        if let Some(row) = row.try_as_sqlite_row() {
            return sqlite::cell_json(row, index, document);
        }
        Err(String::new())
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
            // PostgreSQL and MySQL declare exact numerics, whose declared type
            // reads every digit; `Decimal` rounds past 28.
            ColumnType::Decimal(_) | ColumnType::Money(_) => {
                #[cfg(feature = "sqlite")]
                if row.try_as_sqlite_row().is_some() {
                    return Self::try_get_json::<rust_decimal::Decimal>(row, index);
                }
                None
            }
            ColumnType::Char(_) | ColumnType::String(_) | ColumnType::Text => {
                Self::try_get_json::<String>(row, index)
            }
            ColumnType::Uuid => Self::try_get_json::<uuid::Uuid>(row, index),
            ColumnType::Json | ColumnType::JsonBinary => {
                Self::try_get_json::<serde_json::Value>(row, index)
            }
            ColumnType::Date => Self::try_get_json::<chrono::NaiveDate>(row, index),
            ColumnType::Time => {
                // MySQL's `TIME` is a signed duration, which its declared type
                // reads.
                #[cfg(feature = "mysql")]
                if row.try_as_mysql_row().is_some() {
                    return None;
                }
                Self::try_get_json::<chrono::NaiveTime>(row, index)
            }
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
    /// JSON, so the declared type is read instead of reporting a value that
    /// never existed. Only a real SQL `NULL` yields `Some(Value::Null)`.
    fn option_to_json<T>(value: Option<T>) -> Option<serde_json::Value>
    where
        T: serde::Serialize,
    {
        match value {
            Some(value) => serde_json::to_value(value).ok(),
            None => Some(serde_json::Value::Null),
        }
    }
}
