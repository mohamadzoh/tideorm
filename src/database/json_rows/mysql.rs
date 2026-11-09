use super::Database;
use crate::internal::driver::mysql::MySqlRow;
use crate::internal::driver::mysql::types::{MySqlDecodedValue, MySqlTime};
use crate::internal::driver::{Row, TypeInfo, ValueRef};
use serde_json::Value;

/// Cell `index` of `row` as JSON, or the name of its declared type when the
/// driver cannot decode it.
pub(super) fn cell_json(row: &MySqlRow, index: usize) -> Result<Value, String> {
    let value = row.try_get_raw(index).map_err(|_| String::new())?;
    value
        .decode_lossless()
        .ok()
        .and_then(to_json)
        .ok_or_else(|| value.type_info().name().to_ascii_uppercase())
}

fn to_json(value: MySqlDecodedValue) -> Option<Value> {
    use MySqlDecodedValue as V;
    Some(match value {
        V::Null => Value::Null,
        V::Bool(v) => Value::Bool(v),
        V::Integer(v) => v.into(),
        V::Unsigned(v) => v.into(),
        V::Float(v) => Database::f64_to_json(v),
        V::Decimal(v) | V::Text(v) => Value::String(v),
        V::Bytes(v) => serde_json::to_value(v).ok()?,
        V::Json(v) => v,
        V::Uuid(v) => Value::String(v.to_string()),
        V::Date(v) => serde_json::to_value(v).ok()?,
        V::Time(v) => Value::String(time_text(v)),
        V::DateTime(v) => serde_json::to_value(v).ok()?,
    })
}

/// A MySQL `TIME`, which is a signed duration of up to 838 hours rather than
/// a time of day, as `[-]HH:MM:SS` and any fraction of a second the way a
/// `NaiveTime` writes one, so a time of day reads as one does.
fn time_text(time: MySqlTime) -> String {
    use std::fmt::Write;

    let mut text = format!(
        "{}{:02}:{:02}:{:02}",
        if time.is_negative() { "-" } else { "" },
        time.hours(),
        time.minutes(),
        time.seconds()
    );
    let _ = match time.microseconds() {
        0 => Ok(()),
        micros if micros % 1_000 == 0 => write!(text, ".{:03}", micros / 1_000),
        micros => write!(text, ".{micros:06}"),
    };
    text
}

#[cfg(test)]
#[path = "../../../tests/unit/database_mysql_json_tests.rs"]
mod tests;
