//! Split a `SchemaWriter` export into statements without splitting function
//! bodies, literals or comments.
use tideorm::orm::sql::{Dialect, Kind, tokens};

/// The statements of `sql`. A `DELIMITER` directive switches the separator;
/// otherwise `;` separates, found with MySQL's quoting and comment rules when
/// `mysql` is set and with the rules for any backend when it is not.
pub fn statements(sql: &str, mysql: bool) -> Vec<&str> {
    let mut remaining = sql;
    let mut delimiter = ";";
    let mut result = Vec::new();
    let dialect = if mysql { Dialect::MySql } else { Dialect::Any };
    while !remaining.trim().is_empty() {
        remaining = remaining.trim_start();
        if let Some(directive) = remaining.strip_prefix("DELIMITER ") {
            let end = directive.find('\n').unwrap_or(directive.len());
            delimiter = directive[..end].trim();
            remaining = &directive[end..];
            continue;
        }
        let end = if delimiter != ";" {
            remaining.find(delimiter)
        } else {
            tokens(remaining, dialect)
                .find(|token| token.kind == Kind::Symbol && token.text == ";")
                .map(|token| token.start)
        };
        if let Some(end) = end {
            result.push(&remaining[..end]);
            remaining = &remaining[end + delimiter.len()..];
        } else {
            result.push(remaining);
            break;
        }
    }
    result
}
