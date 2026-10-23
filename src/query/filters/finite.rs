//! Filter values JSON cannot hold.
//!
//! `serde_json` writes a NaN or an infinite float as `null`, and a filter reads
//! `null` as SQL NULL, so `where_eq("price", f64::NAN)` would become
//! `price IS NULL`. Such a value is refused instead. Only a value whose JSON
//! holds a `null` is walked a second time, through
//! [`json_loss`](crate::internal::json_loss), to tell a NaN from a `None`.

use serde::Serialize;

use crate::internal::json_loss;

/// `value` as the JSON a filter carries, or why it cannot be one.
pub(crate) fn checked_filter_value(value: impl Serialize) -> Result<serde_json::Value, String> {
    // A value JSON cannot hold, such as an `i128` past 64 bits, has no
    // parameter to become.
    let json = serde_json::to_value(&value)
        .map_err(|error| format!("a filter value cannot be bound: {error}"))?;
    if holds_null(&json) && json_loss(&value).is_ok_and(|loss| loss.non_finite) {
        return Err(
            "a filter value holds a NaN or infinite float, which SQL cannot compare".to_string(),
        );
    }
    Ok(json)
}

/// Preserve a top-level Option::None before JSON erases its distinction from
/// a JSON null document. Some(null), references and transparent newtypes work
/// through Serde's own representation, without Rust type-name heuristics.
pub(crate) fn checked_assignment_value(
    value: impl Serialize,
) -> Result<(serde_json::Value, bool), String> {
    let json = checked_filter_value(&value)?;
    let sql_null = json.is_null() && json_loss(&value).map_err(|e| e.to_string())?.none;
    Ok((json, sql_null))
}

fn holds_null(json: &serde_json::Value) -> bool {
    match json {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.iter().any(holds_null),
        serde_json::Value::Object(members) => members.values().any(holds_null),
        _ => false,
    }
}
