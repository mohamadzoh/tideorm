//! Containment documents that JSON cannot hold.
//!
//! `serde_json` writes a NaN or an infinite float as `null`, which would be
//! compared as NULL, so such a value is refused instead. Only a
//! value whose JSON holds a `null` is walked a second time, through
//! [`json_loss`](crate::internal::json_loss), to tell a NaN from a `None`.

use serde::Serialize;

use crate::internal::json_loss;

/// `value` as JSON, or why it cannot be.
fn checked_json(value: impl Serialize) -> Result<serde_json::Value, String> {
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

/// `value` as a JSON document, and whether it is a top-level `None` (SQL
/// NULL), which JSON would not tell from the `null` document. `Some(null)`,
/// references and transparent newtypes work through serde's own
/// representation, without Rust type-name heuristics.
pub(super) fn checked_document(value: impl Serialize) -> Result<(serde_json::Value, bool), String> {
    let json = checked_json(&value)?;
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
