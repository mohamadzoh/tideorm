//! PostgreSQL's `@>` and `<@` on MySQL and MariaDB.
//!
//! `JSON_CONTAINS` differs from PostgreSQL in one place: a candidate that is
//! not an array is contained in an array holding it at any depth, so
//! `{"tags": "a"}` is contained in `{"tags": ["a", "b"]}`. The document on the
//! parameter side is known when the statement is built, so each of its object
//! paths adds a check that the column's value there has the kind PostgreSQL
//! requires. Below an array the paths are not known, so everything inside an
//! array keeps MySQL's reading: `[{"k": 1}]` is contained in `[{"k": [1, 2]}]`.
//!
//! The document is bound as JSON text, which `JSON_CONTAINS` parses on MySQL
//! and MariaDB alike; MariaDB has no `CAST(.. AS JSON)`.

use super::{Value, json_scalar_parameter, json_string_contents};

/// `column_sql @> candidate`.
pub(super) fn mysql_json_contains(
    column_sql: &str,
    candidate: &serde_json::Value,
) -> (String, Vec<Value>) {
    let mut guards = Vec::new();
    contains_guards(candidate, "$".to_string(), &mut guards);
    render(
        format!("JSON_CONTAINS({column_sql}, ?)"),
        column_sql,
        candidate,
        guards,
    )
}

/// `column_sql <@ target`.
pub(super) fn mysql_json_contained_by(
    column_sql: &str,
    target: &serde_json::Value,
) -> (String, Vec<Value>) {
    let mut guards = Vec::new();
    contained_by_guards(target, "$".to_string(), &mut guards);
    render(
        format!("JSON_CONTAINS(?, {column_sql})"),
        column_sql,
        target,
        guards,
    )
}

/// What the column's value at a path must be.
#[derive(Clone, Copy)]
enum Kind {
    Object,
    NotArray,
    NotObject,
    ArrayOrAbsent,
}

/// A candidate object only matches an object, and a scalar below the top level
/// never matches an array; only a top-level scalar may match a top-level array.
fn contains_guards(candidate: &serde_json::Value, path: String, guards: &mut Vec<(String, Kind)>) {
    match candidate {
        serde_json::Value::Object(members) => {
            for (key, value) in members {
                contains_guards(value, member_path(&path, key), guards);
            }
            guards.push((path, Kind::Object));
        }
        serde_json::Value::Array(_) => {}
        _ if path == "$" => {}
        _ => guards.push((path, Kind::NotArray)),
    }
}

/// Where the target holds an array below the top level, the column's value
/// there, if it has one, must be an array; at the top level it must not be an
/// object, since only a scalar may be contained in a top-level array.
fn contained_by_guards(target: &serde_json::Value, path: String, guards: &mut Vec<(String, Kind)>) {
    match target {
        serde_json::Value::Object(members) => {
            for (key, value) in members {
                contained_by_guards(value, member_path(&path, key), guards);
            }
        }
        serde_json::Value::Array(_) if path == "$" => guards.push((path, Kind::NotObject)),
        serde_json::Value::Array(_) => guards.push((path, Kind::ArrayOrAbsent)),
        _ => {}
    }
}

fn member_path(path: &str, key: &str) -> String {
    format!("{path}.\"{}\"", json_string_contents(key))
}

/// `containment AND` each guard, binding the document and then each path.
fn render(
    containment: String,
    column_sql: &str,
    document: &serde_json::Value,
    guards: Vec<(String, Kind)>,
) -> (String, Vec<Value>) {
    let mut sql = containment;
    let mut values = vec![json_scalar_parameter(document)];
    for (path, kind) in guards {
        let value_type = format!("JSON_TYPE(JSON_EXTRACT({column_sql}, ?))");
        let check = match kind {
            Kind::Object => format!("{value_type} = 'OBJECT'"),
            Kind::NotArray => format!("{value_type} <> 'ARRAY'"),
            Kind::NotObject => format!("{value_type} <> 'OBJECT'"),
            Kind::ArrayOrAbsent => format!("COALESCE({value_type}, 'ARRAY') = 'ARRAY'"),
        };
        sql.push_str(" AND ");
        sql.push_str(&check);
        values.push(Value::String(Some(path)));
    }
    (sql, values)
}
