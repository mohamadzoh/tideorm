use crate::config::DatabaseType;

fn escape_sql_literal(value: &str) -> String {
    value.replace("'", "''")
}

fn json_string_contents(value: &str) -> String {
    let json = serde_json::to_string(value).expect("serializing JSON path segment should not fail");
    json[1..json.len() - 1].to_string()
}

fn canonical_json_member_path(key: &str) -> String {
    format!("$.\"{}\"", json_string_contents(key))
}

fn normalize_mysql_sqlite_json_path(path: &str) -> Option<String> {
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

fn invalid_json_path_predicate(exists: bool) -> String {
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
    escape_sql_literal(&json)
}

fn mysql_json_scalar_literal(value: &str) -> String {
    let json = serde_json::to_string(&sql_array_value_to_json(value))
        .expect("serializing JSON scalar should not fail");
    escape_sql_literal(&json)
}

fn is_safe_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub(crate) fn validate_identifier(kind: &str, value: &str) -> std::result::Result<(), String> {
    if !value.is_empty() && is_safe_identifier_segment(value) {
        return Ok(());
    }

    Err(format!(
        "unsafe {} '{}': JOIN identifiers may only contain ASCII letters, numbers, and underscores, and must not start with a number",
        kind, value
    ))
}

pub(crate) fn validate_join_column(value: &str) -> std::result::Result<(), String> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() == 2 && parts.iter().all(|part| is_safe_identifier_segment(part)) {
        return Ok(());
    }

    Err(format!(
        "unsafe JOIN column reference '{}': expected table.column using only ASCII letters, numbers, and underscores",
        value
    ))
}

/// Get the identifier quote character for the database
pub fn quote_char(db_type: DatabaseType) -> char {
    match db_type {
        DatabaseType::Postgres | DatabaseType::SQLite => '"',
        DatabaseType::MySQL | DatabaseType::MariaDB => '`',
    }
}

/// Quote an identifier (column or table name)
pub fn quote_ident(db_type: DatabaseType, name: &str) -> String {
    let q = quote_char(db_type);
    let escaped = name.replace(q, &format!("{q}{q}"));
    format!("{}{}{}", q, escaped, q)
}

/// Generate JSON contains expression
///
/// - PostgreSQL: `column @> 'value'`
/// - MySQL: `JSON_CONTAINS(column, 'value')`
/// - SQLite: `json_type(column) IS NOT NULL AND json(column) LIKE '%value%'` (fallback)
pub fn json_contains(db_type: DatabaseType, column: &str, value: &str) -> String {
    let escaped_value = escape_sql_literal(value);
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

/// Generate JSON contained by expression
///
/// - PostgreSQL: `column <@ 'value'`
/// - MySQL: `JSON_CONTAINS('value', column)`
/// - SQLite: Limited support via JSON1
pub fn json_contained_by(db_type: DatabaseType, column: &str, value: &str) -> String {
    let escaped_value = escape_sql_literal(value);
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

/// Generate JSON key exists expression
///
/// - PostgreSQL: `column ? 'key'`
/// - MySQL: `JSON_CONTAINS_PATH(column, 'one', '$.key')`
/// - SQLite: `json_extract(column, '$.key') IS NOT NULL`
pub fn json_key_exists(db_type: DatabaseType, column: &str, key: &str) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_key = escape_sql_literal(key);
            format!("{} ? '{}'", column, escaped_key)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let path = escape_sql_literal(&canonical_json_member_path(key));
            format!("JSON_CONTAINS_PATH({}, 'one', '{}')", column, path)
        }
        DatabaseType::SQLite => {
            let path = escape_sql_literal(&canonical_json_member_path(key));
            format!("json_extract({}, '{}') IS NOT NULL", column, path)
        }
    }
}

/// Generate JSON key not exists expression
pub fn json_key_not_exists(db_type: DatabaseType, column: &str, key: &str) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_key = escape_sql_literal(key);
            format!("NOT ({} ? '{}')", column, escaped_key)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let path = escape_sql_literal(&canonical_json_member_path(key));
            format!(
                "NOT JSON_CONTAINS_PATH({}, 'one', '{}')",
                column, path
            )
        }
        DatabaseType::SQLite => {
            let path = escape_sql_literal(&canonical_json_member_path(key));
            format!("json_extract({}, '{}') IS NULL", column, path)
        }
    }
}

/// Generate JSON path exists expression
///
/// - PostgreSQL: `column @? 'path'`
/// - MySQL: `JSON_CONTAINS_PATH(column, 'one', 'path')`
/// - SQLite: `json_extract(column, 'path') IS NOT NULL`
pub fn json_path_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_path = escape_sql_literal(path);
            format!("{} @? '{}'", column, escaped_path)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(true);
            };
            format!(
                "JSON_CONTAINS_PATH({}, 'one', '{}')",
                column,
                escape_sql_literal(&path)
            )
        }
        DatabaseType::SQLite => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(true);
            };
            format!("json_extract({}, '{}') IS NOT NULL", column, escape_sql_literal(&path))
        }
    }
}

/// Generate JSON path not exists expression
pub fn json_path_not_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            let escaped_path = escape_sql_literal(path);
            format!("NOT ({} @? '{}')", column, escaped_path)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(false);
            };
            format!(
                "NOT JSON_CONTAINS_PATH({}, 'one', '{}')",
                column,
                escape_sql_literal(&path)
            )
        }
        DatabaseType::SQLite => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate(false);
            };
            format!("json_extract({}, '{}') IS NULL", column, escape_sql_literal(&path))
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
            format!("JSON_CONTAINS({}, '{}')", column, mysql_json_array_literal(values))
        }
        DatabaseType::SQLite => {
            let conditions: Vec<String> = values
                .iter()
                .map(|v| {
                    let clean_val = v.trim_matches('\'');
                    format!(
                        "EXISTS (SELECT 1 FROM json_each({}) WHERE value = '{}')",
                        column,
                        escape_sql_literal(clean_val)
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
            format!("JSON_CONTAINS('{}', {})", mysql_json_array_literal(values), column)
        }
        DatabaseType::SQLite => {
            let value_list = values
                .iter()
                .map(|v| format!("'{}'", escape_sql_literal(v.trim_matches('\''))))
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
                .map(|v| format!("JSON_CONTAINS({}, '{}')", column, mysql_json_scalar_literal(v)))
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
                        escape_sql_literal(clean_val)
                    )
                })
                .collect();
            format!("({})", conditions.join(" OR "))
        }
    }
}

/// Format a column identifier for the database
pub fn format_column(db_type: DatabaseType, column: &str) -> String {
    if column.contains('(') || column.contains('*') {
        column.to_string()
    } else if column.contains('.') {
        let parts: Vec<&str> = column.split('.').collect();
        if parts.len() == 2 {
            format!(
                "{}.{}",
                quote_ident(db_type, parts[0]),
                quote_ident(db_type, parts[1])
            )
        } else {
            column.to_string()
        }
    } else {
        quote_ident(db_type, column)
    }
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
