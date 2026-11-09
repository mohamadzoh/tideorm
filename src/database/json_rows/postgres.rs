use super::Database;
use crate::internal::driver::postgres::PgRow;
use crate::internal::driver::postgres::types::PgDecodedValue;
use crate::internal::driver::{Row, TypeInfo, ValueRef};
use serde_json::Value;

/// Cell `index` of `row` as JSON, or the name of its declared type when the
/// driver cannot decode it.
pub(super) fn cell_json(row: &PgRow, index: usize) -> Result<Value, String> {
    let value = row.try_get_raw(index).map_err(|_| String::new())?;
    value
        .decode_lossless()
        .ok()
        .and_then(to_json)
        .ok_or_else(|| value.type_info().name().to_ascii_uppercase())
}

fn to_json(value: PgDecodedValue) -> Option<Value> {
    use PgDecodedValue as V;
    Some(match value {
        V::Null => Value::Null,
        V::Bool(v) => Value::Bool(v),
        V::Integer(v) => v.into(),
        V::Float(v) => Database::f64_to_json(v),
        V::Text(v) | V::Numeric(v) => Value::String(v),
        V::SpecialMoment(v) => Value::String(v.to_owned()),
        V::Bytes(v) => serde_json::to_value(v).ok()?,
        V::Json(v) => v,
        V::Date(v) => serde_json::to_value(v).ok()?,
        V::Time(v) => serde_json::to_value(v).ok()?,
        V::Timestamp(v) => serde_json::to_value(v).ok()?,
        V::TimestampWithTimeZone(v) => serde_json::to_value(v.fixed_offset()).ok()?,
        V::Money(v) => serde_json::to_value(v.to_decimal(2)).ok()?,
        V::Array(v) => {
            if v.dimensions.is_empty() {
                return Some(Value::Array(Vec::new()));
            }
            let mut elements = v.elements.into_iter();
            nest(&v.dimensions, &mut elements)?
        }
    })
}

fn nest(dimensions: &[usize], elements: &mut std::vec::IntoIter<PgDecodedValue>) -> Option<Value> {
    let Some((length, inner)) = dimensions.split_first() else {
        return to_json(elements.next()?);
    };
    (0..*length)
        .map(|_| nest(inner, elements))
        .collect::<Option<Vec<_>>>()
        .map(Value::Array)
}

#[cfg(test)]
#[path = "../../../tests/unit/database_postgres_json_tests.rs"]
mod tests;
