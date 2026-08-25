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

/// Alias given to the `unnest(..)` derived table in the PostgreSQL array
/// renderings, together with the name of its single column.
const POSTGRES_ARRAY_ELEMENT_ALIAS: &str = "tideorm_array_element";

/// Render one array element as the inline SQL literal used by preview SQL.
///
/// The value is escaped exactly once, straight from the original JSON — quote
/// doubling is what PostgreSQL and SQLite need verbatim, and it is also the
/// intermediate form `mysql_json_array_literal` re-encodes for MySQL. Nothing
/// executable goes through here: those paths bind the values as parameters.
fn array_element_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => format!("'{}'", sql_safety::escape_sql_literal(text)),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(boolean) => boolean.to_string(),
        serde_json::Value::Null => "NULL".to_string(),
        other => format!("'{}'", sql_safety::escape_sql_literal(&other.to_string())),
    }
}

fn array_element_literals(values: &[serde_json::Value]) -> Vec<String> {
    values.iter().map(array_element_literal).collect()
}

/// Render PostgreSQL's `column @> ARRAY[..]` without the array constructor.
///
/// `operands` are already-rendered SQL operands: inline literals on the preview
/// path, bound placeholders (`$1`, `$2`, ..) on the executable one. Sharing the
/// shape between both callers is what keeps `debug()` honest about what runs.
///
/// The `ARRAY[..]` constructor is deliberately avoided: sea-query's fragment
/// tokenizer treats `[` as a string delimiter that ends at `]`, so a placeholder
/// written between the brackets is skipped by `Expr::cust_with_values` and its
/// value silently never binds. `x = ANY(column)` is the bracket-free equivalent
/// and additionally lets PostgreSQL infer each parameter's type from the
/// column's element type.
///
/// An empty operand list is vacuously contained, so it renders as true.
pub(crate) fn postgres_array_contains(column: &str, operands: &[String]) -> String {
    postgres_array_element_match(column, operands, " AND ", "1 = 1")
}

/// Render PostgreSQL's `column && ARRAY[..]` (overlap) without the array
/// constructor. See [`postgres_array_contains`] for why the brackets are gone.
///
/// An empty operand list cannot overlap with anything, so it renders as false.
pub(crate) fn postgres_array_overlaps(column: &str, operands: &[String]) -> String {
    postgres_array_element_match(column, operands, " OR ", "0 = 1")
}

fn postgres_array_element_match(
    column: &str,
    operands: &[String],
    combine: &str,
    empty_result: &str,
) -> String {
    if operands.is_empty() {
        return empty_result.to_string();
    }

    let checks: Vec<String> = operands
        .iter()
        .map(|operand| format!("{} = ANY({})", operand, column))
        .collect();
    format!("({})", checks.join(combine))
}

/// Render PostgreSQL's `column <@ ARRAY[..]` (contained by) without the array
/// constructor. See [`postgres_array_contains`] for why the brackets are gone.
///
/// An empty operand list leaves only the empty array contained, which is exactly
/// what the element-free `NOT EXISTS` renders.
pub(crate) fn postgres_array_contained_by(column: &str, operands: &[String]) -> String {
    let alias = POSTGRES_ARRAY_ELEMENT_ALIAS;
    let source = format!("unnest({}) AS {}(element)", column, alias);

    if operands.is_empty() {
        return format!("NOT EXISTS (SELECT 1 FROM {})", source);
    }

    let elements = operands.join(", ");
    format!("NOT EXISTS (SELECT 1 FROM {source} WHERE {alias}.element NOT IN ({elements}))")
}

/// Render non-executable preview SQL for an array contains expression.
///
/// - PostgreSQL: one `= ANY(column)` check per element, combined with `AND`
/// - MySQL: `JSON_CONTAINS` against a JSON array
/// - SQLite: one `json_each` existence check per element, combined with `AND`
///
/// An empty candidate list is vacuously contained on every backend.
pub fn array_contains(db_type: DatabaseType, column: &str, values: &[serde_json::Value]) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => postgres_array_contains(&column, &array_element_literals(values)),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "JSON_CONTAINS({}, '{}')",
                column,
                mysql_json_array_literal(&array_element_literals(values))
            )
        }
        DatabaseType::SQLite => {
            if values.is_empty() {
                return "1 = 1".to_string();
            }

            let conditions: Vec<String> = values
                .iter()
                .map(|value| sqlite_json_each_match(&column, value))
                .collect();
            format!("({})", conditions.join(" AND "))
        }
    }
}

/// Render non-executable preview SQL for an array contained-by expression.
///
/// An empty candidate list only contains the empty array, which every backend
/// renders as an emptiness check on the column.
pub fn array_contained_by(
    db_type: DatabaseType,
    column: &str,
    values: &[serde_json::Value],
) -> String {
    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => {
            postgres_array_contained_by(&column, &array_element_literals(values))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "JSON_CONTAINS('{}', {})",
                mysql_json_array_literal(&array_element_literals(values)),
                column
            )
        }
        DatabaseType::SQLite => {
            if values.is_empty() {
                return format!("NOT EXISTS (SELECT 1 FROM json_each({}))", column);
            }

            format!(
                "NOT EXISTS (SELECT 1 FROM json_each({}) WHERE value NOT IN ({}))",
                column,
                array_element_literals(values).join(", ")
            )
        }
    }
}

/// Render non-executable preview SQL for an array overlaps expression (any
/// element matches).
///
/// An empty candidate list can never overlap, so every backend renders false.
pub fn array_overlaps(db_type: DatabaseType, column: &str, values: &[serde_json::Value]) -> String {
    if values.is_empty() {
        return "0 = 1".to_string();
    }

    let column = format_column(db_type, column);
    match db_type {
        DatabaseType::Postgres => postgres_array_overlaps(&column, &array_element_literals(values)),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let conditions: Vec<String> = values
                .iter()
                .map(|value| {
                    format!(
                        "JSON_CONTAINS({}, '{}')",
                        column,
                        mysql_json_scalar_literal(&array_element_literal(value))
                    )
                })
                .collect();
            format!("({})", conditions.join(" OR "))
        }
        DatabaseType::SQLite => {
            let conditions: Vec<String> = values
                .iter()
                .map(|value| sqlite_json_each_match(&column, value))
                .collect();
            format!("({})", conditions.join(" OR "))
        }
    }
}

fn sqlite_json_each_match(column: &str, value: &serde_json::Value) -> String {
    format!(
        "EXISTS (SELECT 1 FROM json_each({}) WHERE value = {})",
        column,
        array_element_literal(value)
    )
}
