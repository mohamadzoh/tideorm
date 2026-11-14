//! The model rules that read a column's type, which only the compiler knows:
//! the derive writes `const` items calling these with each field's
//! `ValueType::COLUMN_KIND` and panicking with the message they return, so a
//! broken rule fails `cargo check`.

use super::message::Message;
use crate::orm::sql_builder::ColumnKind;

/// What is broken when `model` has more than `max` JSON columns.
#[doc(hidden)]
pub const fn __check_json_columns(
    model: &str,
    columns: &[ColumnKind],
    max: usize,
) -> Option<Message> {
    let mut count = 0;
    let mut at = 0;
    while at < columns.len() {
        if matches!(columns[at], ColumnKind::Json) {
            count += 1;
        }
        at += 1;
    }
    if count <= max {
        return None;
    }
    let mut message = Message::new();
    message
        .push(model)
        .push(" has ")
        .number(count)
        .push(" JSON columns, over the limit of ")
        .number(max);
    Some(*message.rule("max_json_columns"))
}

/// What is broken when `model`'s `column` holds floating-point numbers.
#[doc(hidden)]
pub const fn __check_float_column(model: &str, column: &str, kind: ColumnKind) -> Option<Message> {
    if !matches!(kind, ColumnKind::Float) {
        return None;
    }
    let mut message = Message::new();
    message
        .push(model)
        .push("'s column `")
        .push(column)
        .push("` is a floating-point number; use a decimal or an integer");
    Some(*message.rule("forbid_float_columns"))
}

/// What is broken when `model`'s primary key `column` is none of `allowed`,
/// as `primary_key_types` names key types.
#[doc(hidden)]
pub const fn __check_key_type(
    model: &str,
    column: &str,
    kind: ColumnKind,
    allowed: &[&str],
) -> Option<Message> {
    let name = match kind {
        ColumnKind::Integer => "integer",
        ColumnKind::BigInteger => "bigint",
        ColumnKind::Uuid => "uuid",
        ColumnKind::Text => "string",
        _ => "another type",
    };
    let mut at = 0;
    while at < allowed.len() {
        if crate::model::__str_eq(allowed[at], name) {
            return None;
        }
        at += 1;
    }
    let mut message = Message::new();
    message
        .push(model)
        .push("'s primary key `")
        .push(column)
        .push("` is ")
        .push(name)
        .push(", not one of ");
    let mut at = 0;
    while at < allowed.len() {
        if at > 0 {
            message.push(", ");
        }
        message.push(allowed[at]);
        at += 1;
    }
    Some(*message.rule("primary_key_types"))
}
