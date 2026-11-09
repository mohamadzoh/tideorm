use super::Database;
use crate::internal::driver::sqlite::SqliteRow;
use crate::internal::driver::sqlite::types::SqliteDecodedValue;
use crate::internal::driver::{Row, TypeInfo, ValueRef};
use serde_json::Value;

/// Cell `index` of `row` as JSON, or the name of what SQLite stores there
/// when the driver cannot decode it. A `document` column's text is read as
/// the object or array it holds; any other text is a string, however it
/// looks.
pub(super) fn cell_json(row: &SqliteRow, index: usize, document: bool) -> Result<Value, String> {
    let value = row.try_get_raw(index).map_err(|_| String::new())?;
    value
        .decode_lossless()
        .ok()
        .and_then(|decoded| to_json(decoded, document))
        .ok_or_else(|| value.type_info().name().to_ascii_uppercase())
}

/// A real is the number SQLite stores, as from `SUM()` over a `REAL` column
/// or from a `DECIMAL` column: the driver cannot tell the two apart, and a
/// decimal would turn a float sum into a string. A model query still reads
/// its own `Decimal` fields as decimals.
fn to_json(value: SqliteDecodedValue, document: bool) -> Option<Value> {
    use SqliteDecodedValue as V;
    Some(match value {
        V::Null => Value::Null,
        V::Bool(v) => Value::Bool(v),
        V::Integer(v) => v.into(),
        V::Real(v) => Database::f64_to_json(v),
        V::Text(text) => document
            .then(|| serde_json::from_str::<Value>(&text).ok())
            .flatten()
            .filter(|document| document.is_object() || document.is_array())
            .unwrap_or(Value::String(text)),
        V::Blob(v) => serde_json::to_value(v).ok()?,
        V::Date(v) => serde_json::to_value(v).ok()?,
        V::Time(v) => serde_json::to_value(v).ok()?,
        V::DateTime(v) => serde_json::to_value(v).ok()?,
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/database_sqlite_json_tests.rs"]
mod tests;
