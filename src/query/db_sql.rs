use crate::config::DatabaseType;
use crate::internal::Value;
use crate::internal::sql_safety;

fn escape_sql_literal(db_type: DatabaseType, value: &str) -> String {
    sql_safety::escape_sql_literal_for_db(db_type, value)
}

fn escape_mysql_literal(value: &str) -> String {
    escape_sql_literal(DatabaseType::MySQL, value)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundSql {
    pub sql: String,
    pub values: Vec<Value>,
}

impl BoundSql {
    fn new(sql: String, values: Vec<Value>) -> Self {
        Self { sql, values }
    }
}

fn json_text_value(text: String) -> Value {
    Value::String(Some(text))
}

fn json_scalar_parameter(value: &serde_json::Value) -> Value {
    json_text_value(
        serde_json::to_string(value).expect("serializing scalar predicate value should not fail"),
    )
}

fn json_native_parameter(value: &serde_json::Value) -> Value {
    Value::Json(Some(Box::new(value.clone())))
}

fn sqlite_json_compare_parameter(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::String(text) => Value::String(Some(text.clone())),
        serde_json::Value::Null => Value::String(Some("null".to_string())),
        serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Value::BigInt(Some(integer))
            } else if let Some(float) = number.as_f64() {
                Value::Double(Some(float))
            } else {
                Value::String(Some(number.to_string()))
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Value::String(Some(value.to_string()))
        }
    }
}

fn json_string_contents(value: &str) -> String {
    let json = serde_json::to_string(value).expect("serializing JSON path segment should not fail");
    json[1..json.len() - 1].to_string()
}

pub(crate) fn canonical_json_member_path(key: &str) -> String {
    format!("$.\"{}\"", json_string_contents(key))
}

pub(crate) fn json_contains_bound(
    db_type: DatabaseType,
    column_sql: &str,
    value: &serde_json::Value,
) -> BoundSql {
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("{} @> $1", column_sql),
            vec![json_native_parameter(value)],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => BoundSql::new(
            format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
            vec![json_scalar_parameter(value)],
        ),
        DatabaseType::SQLite => BoundSql::new(
            format!(
                "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                column_sql
            ),
            vec![sqlite_json_compare_parameter(value)],
        ),
    }
}

pub(crate) fn json_contained_by_bound(
    db_type: DatabaseType,
    column_sql: &str,
    value: &serde_json::Value,
) -> BoundSql {
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("{} <@ $1", column_sql),
            vec![json_native_parameter(value)],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => BoundSql::new(
            format!("JSON_CONTAINS(CAST(? AS JSON), {})", column_sql),
            vec![json_scalar_parameter(value)],
        ),
        DatabaseType::SQLite => BoundSql::new(
            format!(
                "json_type({}) IS NOT NULL AND ? LIKE '%' || {} || '%'",
                column_sql, column_sql
            ),
            vec![json_scalar_parameter(value)],
        ),
    }
}

pub(crate) fn json_key_exists_bound(
    db_type: DatabaseType,
    column_sql: &str,
    key: &str,
) -> BoundSql {
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("{} ? $1", column_sql),
            vec![Value::String(Some(key.to_string()))],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => BoundSql::new(
            format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
            vec![Value::String(Some(canonical_json_member_path(key)))],
        ),
        DatabaseType::SQLite => BoundSql::new(
            format!("json_extract({}, ?) IS NOT NULL", column_sql),
            vec![Value::String(Some(canonical_json_member_path(key)))],
        ),
    }
}

pub(crate) fn json_key_not_exists_bound(
    db_type: DatabaseType,
    column_sql: &str,
    key: &str,
) -> BoundSql {
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("NOT ({} ? $1)", column_sql),
            vec![Value::String(Some(key.to_string()))],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => BoundSql::new(
            format!("NOT JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
            vec![Value::String(Some(canonical_json_member_path(key)))],
        ),
        DatabaseType::SQLite => BoundSql::new(
            format!("json_extract({}, ?) IS NULL", column_sql),
            vec![Value::String(Some(canonical_json_member_path(key)))],
        ),
    }
}

pub(crate) fn json_path_exists_bound(
    db_type: DatabaseType,
    column_sql: &str,
    path: &str,
) -> Option<BoundSql> {
    match db_type {
        DatabaseType::Postgres => Some(BoundSql::new(
            format!("{} @? ($1::jsonpath)", column_sql),
            vec![Value::String(Some(path.to_string()))],
        )),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            normalize_mysql_sqlite_json_path(path).map(|normalized| {
                BoundSql::new(
                    format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                    vec![Value::String(Some(normalized))],
                )
            })
        }
        DatabaseType::SQLite => normalize_mysql_sqlite_json_path(path).map(|normalized| {
            BoundSql::new(
                format!("json_extract({}, ?) IS NOT NULL", column_sql),
                vec![Value::String(Some(normalized))],
            )
        }),
    }
}

pub(crate) fn json_path_not_exists_bound(
    db_type: DatabaseType,
    column_sql: &str,
    path: &str,
) -> Option<BoundSql> {
    match db_type {
        DatabaseType::Postgres => Some(BoundSql::new(
            format!("NOT ({} @? ($1::jsonpath))", column_sql),
            vec![Value::String(Some(path.to_string()))],
        )),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            normalize_mysql_sqlite_json_path(path).map(|normalized| {
                BoundSql::new(
                    format!("NOT JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                    vec![Value::String(Some(normalized))],
                )
            })
        }
        DatabaseType::SQLite => normalize_mysql_sqlite_json_path(path).map(|normalized| {
            BoundSql::new(
                format!("json_extract({}, ?) IS NULL", column_sql),
                vec![Value::String(Some(normalized))],
            )
        }),
    }
}

pub(crate) fn normalize_mysql_sqlite_json_path(path: &str) -> Option<String> {
    let chars: Vec<char> = path.chars().collect();
    if chars.first().copied() != Some('$') {
        return None;
    }

    let mut index = 1;
    let mut normalized = String::from("$");

    while index < chars.len() {
        match chars[index] {
            '.' => {
                index += 1;
                if index >= chars.len() {
                    return None;
                }

                let segment = if chars[index] == '"' || chars[index] == '\'' {
                    parse_quoted_json_path_segment(&chars, &mut index)?
                } else {
                    let start = index;
                    while index < chars.len() && chars[index] != '.' && chars[index] != '[' {
                        index += 1;
                    }
                    let segment: String = chars[start..index].iter().collect();
                    if !is_safe_identifier_segment(&segment) {
                        return None;
                    }
                    segment
                };

                normalized.push_str(&format!(".\"{}\"", json_string_contents(&segment)));
            }
            '[' => {
                index += 1;
                if index >= chars.len() {
                    return None;
                }

                if chars[index].is_ascii_digit() {
                    let start = index;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                    if index >= chars.len() || chars[index] != ']' {
                        return None;
                    }
                    normalized.push('[');
                    normalized.extend(chars[start..index].iter());
                    normalized.push(']');
                    index += 1;
                } else if chars[index] == '"' || chars[index] == '\'' {
                    let segment = parse_quoted_json_path_segment(&chars, &mut index)?;
                    if index >= chars.len() || chars[index] != ']' {
                        return None;
                    }
                    normalized.push_str(&format!(".\"{}\"", json_string_contents(&segment)));
                    index += 1;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }

    Some(normalized)
}

fn parse_quoted_json_path_segment(chars: &[char], index: &mut usize) -> Option<String> {
    let quote = chars.get(*index).copied()?;
    *index += 1;

    let mut segment = String::new();
    while *index < chars.len() {
        match chars[*index] {
            '\\' => {
                *index += 1;
                let escaped = chars.get(*index).copied()?;
                segment.push(escaped);
                *index += 1;
            }
            ch if ch == quote => {
                *index += 1;
                return Some(segment);
            }
            ch => {
                segment.push(ch);
                *index += 1;
            }
        }
    }

    None
}

pub(crate) fn invalid_json_path_predicate(exists: bool) -> String {
    let _ = exists;
    "0 = 1".to_string()
}

fn sql_array_value_to_json(value: &str) -> serde_json::Value {
    let trimmed = value.trim();
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        return serde_json::Value::String(trimmed[1..trimmed.len() - 1].replace("''", "'"));
    }

    match trimmed {
        "null" | "NULL" => serde_json::Value::Null,
        "true" | "TRUE" => serde_json::Value::Bool(true),
        "false" | "FALSE" => serde_json::Value::Bool(false),
        _ => serde_json::from_str(trimmed)
            .unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string())),
    }
}

fn mysql_json_array_literal(values: &[String]) -> String {
    let json = serde_json::to_string(
        &values
            .iter()
            .map(|value| sql_array_value_to_json(value))
            .collect::<Vec<_>>(),
    )
    .expect("serializing JSON array should not fail");
    escape_mysql_literal(&json)
}

fn mysql_json_scalar_literal(value: &str) -> String {
    let json = serde_json::to_string(&sql_array_value_to_json(value))
        .expect("serializing JSON scalar should not fail");
    escape_mysql_literal(&json)
}

fn is_safe_identifier_segment(segment: &str) -> bool {
    sql_safety::is_safe_identifier_segment(segment)
}

pub(crate) fn validate_raw_sql_fragment(kind: &str, sql: &str) -> std::result::Result<(), String> {
    sql_safety::validate_raw_sql_fragment(kind, sql)
}

pub(crate) fn validate_having_sql_fragment(
    kind: &str,
    sql: &str,
) -> std::result::Result<(), String> {
    sql_safety::validate_having_sql_fragment(kind, sql)
}

pub(crate) fn validate_subquery_sql(sql: &str) -> std::result::Result<(), String> {
    sql_safety::validate_subquery_sql(sql)
}

pub(crate) fn validate_compound_subquery_sql(sql: &str) -> std::result::Result<(), String> {
    sql_safety::validate_compound_subquery_sql(sql)
}

pub(crate) fn validate_identifier(kind: &str, value: &str) -> std::result::Result<(), String> {
    sql_safety::validate_identifier(kind, value)
}

pub(crate) fn validate_identifier_reference(
    kind: &str,
    value: &str,
) -> std::result::Result<(), String> {
    sql_safety::validate_identifier_reference(kind, value)
}

pub(crate) fn validate_join_column(value: &str) -> std::result::Result<(), String> {
    sql_safety::validate_join_column(value)
}

/// Get the identifier quote character for the database
#[cfg(test)]
pub(crate) fn quote_char(db_type: DatabaseType) -> char {
    sql_safety::quote_char(db_type)
}

/// Quote an identifier (column or table name)
pub fn quote_ident(db_type: DatabaseType, name: &str) -> String {
    sql_safety::quote_ident(db_type, name)
}

/// Quote a simple identifier reference like `column` or `table.column`.
pub fn format_identifier_reference(db_type: DatabaseType, value: &str) -> Option<String> {
    sql_safety::format_identifier_reference(db_type, value)
}

/// Format a trusted column/expression slot for rendering paths that intentionally
/// allow raw SQL expressions after higher-level validation.
pub(crate) fn format_column_or_trusted_expression(
    db_type: DatabaseType,
    column_or_expression: &str,
) -> String {
    let trimmed = column_or_expression.trim();
    format_identifier_reference(db_type, trimmed).unwrap_or_else(|| trimmed.to_string())
}

/// Render non-executable preview SQL for a JSON contains expression.
///
/// - PostgreSQL: `column @> 'value'`
/// - MySQL: `JSON_CONTAINS(column, 'value')`
/// - SQLite: `json_type(column) IS NOT NULL AND json(column) LIKE '%value%'` (fallback)
pub(crate) fn preview_json_contains(db_type: DatabaseType, column: &str, value: &str) -> String {
    let escaped_value = escape_sql_literal(db_type, value);
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            format!("{} @> '{}'", column, escaped_value)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!("JSON_CONTAINS({}, '{}')", column, escaped_value)
        }
        DatabaseType::SQLite => {
            format!(
                "EXISTS (SELECT 1 FROM json_each({}) WHERE value = '{}')",
                column,
                escaped_value.trim_matches('"')
            )
        }
    }
}

/// Render non-executable preview SQL for a JSON contained-by expression.
///
/// - PostgreSQL: `column <@ 'value'`
/// - MySQL: `JSON_CONTAINS('value', column)`
/// - SQLite: Limited support via JSON1
pub(crate) fn preview_json_contained_by(
    db_type: DatabaseType,
    column: &str,
    value: &str,
) -> String {
    let escaped_value = escape_sql_literal(db_type, value);
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            format!("{} <@ '{}'", column, escaped_value)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!("JSON_CONTAINS('{}', {})", escaped_value, column)
        }
        DatabaseType::SQLite => {
            format!(
                "json_type({}) IS NOT NULL AND '{}' LIKE '%' || {} || '%'",
                column, escaped_value, column
            )
        }
    }
}

/// Render non-executable preview SQL for a JSON key-exists expression.
///
/// - PostgreSQL: `column ? 'key'`
/// - MySQL: `JSON_CONTAINS_PATH(column, 'one', '$.key')`
/// - SQLite: `json_extract(column, '$.key') IS NOT NULL`
pub(crate) fn preview_json_key_exists(db_type: DatabaseType, column: &str, key: &str) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_key = escape_sql_literal(DatabaseType::Postgres, key);
            format!("{} ? '{}'", column, escaped_key)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let path = escape_sql_literal(db_type, &canonical_json_member_path(key));
            format!("JSON_CONTAINS_PATH({}, 'one', '{}')", column, path)
        }
        DatabaseType::SQLite => {
            let path = escape_sql_literal(DatabaseType::SQLite, &canonical_json_member_path(key));
            format!("json_extract({}, '{}') IS NOT NULL", column, path)
        }
    }
}

/// Render non-executable preview SQL for a JSON key-not-exists expression.
pub(crate) fn preview_json_key_not_exists(
    db_type: DatabaseType,
    column: &str,
    key: &str,
) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_key = escape_sql_literal(DatabaseType::Postgres, key);
            format!("NOT ({} ? '{}')", column, escaped_key)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let path = escape_sql_literal(db_type, &canonical_json_member_path(key));
            format!("NOT JSON_CONTAINS_PATH({}, 'one', '{}')", column, path)
        }
        DatabaseType::SQLite => {
            let path = escape_sql_literal(DatabaseType::SQLite, &canonical_json_member_path(key));
            format!("json_extract({}, '{}') IS NULL", column, path)
        }
    }
}

/// Render non-executable preview SQL for a JSON path-exists expression.
///
/// - PostgreSQL: `column @? 'path'`
/// - MySQL: `JSON_CONTAINS_PATH(column, 'one', 'path')`
/// - SQLite: `json_extract(column, 'path') IS NOT NULL`
pub(crate) fn preview_json_path_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_path = escape_sql_literal(DatabaseType::Postgres, path);
            format!("{} @? '{}'", column, escaped_path)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(true);
            };
            format!(
                "JSON_CONTAINS_PATH({}, 'one', '{}')",
                column,
                escape_sql_literal(db_type, &path)
            )
        }
        DatabaseType::SQLite => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(true);
            };
            format!(
                "json_extract({}, '{}') IS NOT NULL",
                column,
                escape_sql_literal(DatabaseType::SQLite, &path)
            )
        }
    }
}

/// Render non-executable preview SQL for a JSON path-not-exists expression.
pub(crate) fn preview_json_path_not_exists(
    db_type: DatabaseType,
    column: &str,
    path: &str,
) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_path = escape_sql_literal(DatabaseType::Postgres, path);
            format!("NOT ({} @? '{}')", column, escaped_path)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(false);
            };
            format!(
                "NOT JSON_CONTAINS_PATH({}, 'one', '{}')",
                column,
                escape_sql_literal(db_type, &path)
            )
        }
        DatabaseType::SQLite => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(false);
            };
            format!(
                "json_extract({}, '{}') IS NULL",
                column,
                escape_sql_literal(DatabaseType::SQLite, &path)
            )
        }
    }
}

/// Generate array contains expression
///
/// - PostgreSQL: `column @> ARRAY[values]`
/// - MySQL: Uses JSON_CONTAINS with JSON array
/// - SQLite: Uses json_each for array element checking
pub fn array_contains(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            format!("{} @> ARRAY[{}]", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "JSON_CONTAINS({}, '{}')",
                column,
                mysql_json_array_literal(values)
            )
        }
        DatabaseType::SQLite => {
            let conditions: Vec<String> = values
                .iter()
                .map(|v| {
                    let clean_val = v.trim_matches('\'');
                    format!(
                        "EXISTS (SELECT 1 FROM json_each({}) WHERE value = '{}')",
                        column,
                        escape_sql_literal(DatabaseType::SQLite, clean_val)
                    )
                })
                .collect();
            format!("({})", conditions.join(" AND "))
        }
    }
}

/// Generate array contained by expression
pub fn array_contained_by(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            format!("{} <@ ARRAY[{}]", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "JSON_CONTAINS('{}', {})",
                mysql_json_array_literal(values),
                column
            )
        }
        DatabaseType::SQLite => {
            let value_list = values
                .iter()
                .map(|v| {
                    format!(
                        "'{}'",
                        escape_sql_literal(DatabaseType::SQLite, v.trim_matches('\''))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "NOT EXISTS (SELECT 1 FROM json_each({}) WHERE value NOT IN ({}))",
                column, value_list
            )
        }
    }
}

/// Generate array overlaps expression (any element matches)
pub fn array_overlaps(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            format!("{} && ARRAY[{}]", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let conditions: Vec<String> = values
                .iter()
                .map(|v| {
                    format!(
                        "JSON_CONTAINS({}, '{}')",
                        column,
                        mysql_json_scalar_literal(v)
                    )
                })
                .collect();
            format!("({})", conditions.join(" OR "))
        }
        DatabaseType::SQLite => {
            let conditions: Vec<String> = values
                .iter()
                .map(|v| {
                    let clean_val = v.trim_matches('\'');
                    format!(
                        "EXISTS (SELECT 1 FROM json_each({}) WHERE value = '{}')",
                        column,
                        escape_sql_literal(DatabaseType::SQLite, clean_val)
                    )
                })
                .collect();
            format!("({})", conditions.join(" OR "))
        }
    }
}

/// Format a column identifier for the database.
///
/// This helper is intentionally strict: if the input is not a simple
/// identifier reference like `column` or `table.column`, it is quoted as a
/// single identifier instead of being passed through as raw SQL. Call
/// `format_column_or_trusted_expression()` only from rendering paths that
/// intentionally support validated raw expressions.
pub fn format_column(db_type: DatabaseType, column: &str) -> String {
    let trimmed = column.trim();
    format_identifier_reference(db_type, trimmed).unwrap_or_else(|| quote_ident(db_type, trimmed))
}

/// Generate aggregate function with proper casting for the database
pub fn cast_to_float(db_type: DatabaseType, expr: &str) -> String {
    match db_type {
        DatabaseType::Postgres => format!("CAST({} AS FLOAT8)", expr),
        DatabaseType::MySQL | DatabaseType::MariaDB => format!("CAST({} AS DOUBLE)", expr),
        DatabaseType::SQLite => format!("CAST({} AS REAL)", expr),
    }
}

/// Generate = ANY(array) expression (PostgreSQL optimization for IN)
pub fn eq_any(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    match db_type {
        DatabaseType::Postgres => {
            format!("{} = ANY(ARRAY[{}])", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
            format!("{} IN ({})", column, values.join(","))
        }
    }
}

/// Generate <> ALL(array) expression (PostgreSQL optimization for NOT IN)
pub fn ne_all(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    match db_type {
        DatabaseType::Postgres => {
            format!("{} <> ALL(ARRAY[{}])", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
            format!("{} NOT IN ({})", column, values.join(","))
        }
    }
}
