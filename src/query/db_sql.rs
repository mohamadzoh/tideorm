use crate::config::DatabaseType;

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
    format!("{}{}{}", q, name, q)
}

/// Generate JSON contains expression
///
/// - PostgreSQL: `column @> 'value'`
/// - MySQL: `JSON_CONTAINS(column, 'value')`
/// - SQLite: `json_type(column) IS NOT NULL AND json(column) LIKE '%value%'` (fallback)
pub fn json_contains(db_type: DatabaseType, column: &str, value: &str) -> String {
    let escaped_value = value.replace("'", "''");
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" @> '{}'", column, escaped_value)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!("JSON_CONTAINS(`{}`, '{}')", column, escaped_value)
        }
        DatabaseType::SQLite => {
            format!(
                "EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value = '{}')",
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
    let escaped_value = value.replace("'", "''");
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" <@ '{}'", column, escaped_value)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!("JSON_CONTAINS('{}', `{}`)", escaped_value, column)
        }
        DatabaseType::SQLite => {
            format!(
                "json_type(\"{}\") IS NOT NULL AND '{}' LIKE '%' || \"{}\" || '%'",
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
    let escaped_key = key.replace("'", "''");
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" ? '{}'", column, escaped_key)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "JSON_CONTAINS_PATH(`{}`, 'one', '$.{}')",
                column, escaped_key
            )
        }
        DatabaseType::SQLite => {
            format!(
                "json_extract(\"{}\", '$.{}') IS NOT NULL",
                column, escaped_key
            )
        }
    }
}

/// Generate JSON key not exists expression
pub fn json_key_not_exists(db_type: DatabaseType, column: &str, key: &str) -> String {
    let escaped_key = key.replace("'", "''");
    match db_type {
        DatabaseType::Postgres => {
            format!("NOT (\"{}\" ? '{}')", column, escaped_key)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "NOT JSON_CONTAINS_PATH(`{}`, 'one', '$.{}')",
                column, escaped_key
            )
        }
        DatabaseType::SQLite => {
            format!("json_extract(\"{}\", '$.{}') IS NULL", column, escaped_key)
        }
    }
}

/// Generate JSON path exists expression
///
/// - PostgreSQL: `column @? 'path'`
/// - MySQL: `JSON_CONTAINS_PATH(column, 'one', 'path')`
/// - SQLite: `json_extract(column, 'path') IS NOT NULL`
pub fn json_path_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
    let escaped_path = path.replace("'", "''");
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" @? '{}'", column, escaped_path)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "JSON_CONTAINS_PATH(`{}`, 'one', '{}')",
                column, escaped_path
            )
        }
        DatabaseType::SQLite => {
            format!(
                "json_extract(\"{}\", '{}') IS NOT NULL",
                column, escaped_path
            )
        }
    }
}

/// Generate JSON path not exists expression
pub fn json_path_not_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
    let escaped_path = path.replace("'", "''");
    match db_type {
        DatabaseType::Postgres => {
            format!("NOT (\"{}\" @? '{}')", column, escaped_path)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!(
                "NOT JSON_CONTAINS_PATH(`{}`, 'one', '{}')",
                column, escaped_path
            )
        }
        DatabaseType::SQLite => {
            format!("json_extract(\"{}\", '{}') IS NULL", column, escaped_path)
        }
    }
}

/// Generate array contains expression
///
/// - PostgreSQL: `column @> ARRAY[values]`
/// - MySQL: Uses JSON_CONTAINS with JSON array
/// - SQLite: Uses json_each for array element checking
pub fn array_contains(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" @> ARRAY[{}]", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let json_array = format!(
                "[{}]",
                values
                    .iter()
                    .map(|v| if v.starts_with("'") {
                        v.clone()
                    } else {
                        format!("\"{}\"", v.trim_matches('\''))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            );
            format!(
                "JSON_CONTAINS(`{}`, '{}')",
                column,
                json_array.replace("'", "''")
            )
        }
        DatabaseType::SQLite => {
            let conditions: Vec<String> = values
                .iter()
                .map(|v| {
                    let clean_val = v.trim_matches('\'');
                    format!(
                        "EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value = '{}')",
                        column,
                        clean_val.replace("'", "''")
                    )
                })
                .collect();
            format!("({})", conditions.join(" AND "))
        }
    }
}

/// Generate array contained by expression
pub fn array_contained_by(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" <@ ARRAY[{}]", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let json_array = format!(
                "[{}]",
                values
                    .iter()
                    .map(|v| if v.starts_with("'") {
                        v.clone()
                    } else {
                        format!("\"{}\"", v.trim_matches('\''))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            );
            format!(
                "JSON_CONTAINS('{}', `{}`)",
                json_array.replace("'", "''"),
                column
            )
        }
        DatabaseType::SQLite => {
            let value_list = values
                .iter()
                .map(|v| format!("'{}'", v.trim_matches('\'').replace("'", "''")))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "NOT EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value NOT IN ({}))",
                column, value_list
            )
        }
    }
}

/// Generate array overlaps expression (any element matches)
pub fn array_overlaps(db_type: DatabaseType, column: &str, values: &[String]) -> String {
    match db_type {
        DatabaseType::Postgres => {
            format!("\"{}\" && ARRAY[{}]", column, values.join(","))
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let conditions: Vec<String> = values
                .iter()
                .map(|v| {
                    let clean_val = v.trim_matches('\'');
                    format!(
                        "JSON_CONTAINS(`{}`, '\"{}\"')",
                        column,
                        clean_val.replace("'", "''")
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
                        "EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value = '{}')",
                        column,
                        clean_val.replace("'", "''")
                    )
                })
                .collect();
            format!("({})", conditions.join(" OR "))
        }
    }
}

/// Format a column identifier for the database
pub fn format_column(db_type: DatabaseType, column: &str) -> String {
    if column.contains('(') || column.contains('*') || column.contains('"') || column.contains('`')
    {
        column.to_string()
    } else if column.contains('.') {
        let parts: Vec<&str> = column.split('.').collect();
        if parts.len() == 2 {
            let q = quote_char(db_type);
            format!("{0}{1}{0}.{0}{2}{0}", q, parts[0], parts[1])
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
