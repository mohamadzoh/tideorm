use super::*;

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
                return invalid_json_path_predicate();
            };
            format!(
                "JSON_CONTAINS_PATH({}, 'one', '{}')",
                column,
                escape_sql_literal(db_type, &path)
            )
        }
        DatabaseType::SQLite => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate();
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
                return invalid_json_path_predicate();
            };
            format!(
                "NOT JSON_CONTAINS_PATH({}, 'one', '{}')",
                column,
                escape_sql_literal(db_type, &path)
            )
        }
        DatabaseType::SQLite => {
            let Some(path) = normalize_mysql_sqlite_json_path(path) else {
                return invalid_json_path_predicate();
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
