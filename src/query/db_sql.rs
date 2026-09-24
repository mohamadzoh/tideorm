use crate::config::DatabaseType;
use crate::internal::Value;

mod arrays;
mod mysql_json;
mod placeholders;
mod sqlite_json;

pub(crate) use crate::internal::placeholder;
pub(crate) use crate::internal::sql_safety::{
    format_identifier_reference, is_safe_identifier_segment, push_quoted_ident, quote_ident,
    validate_compound_subquery_sql, validate_having_sql_fragment, validate_identifier,
    validate_identifier_reference, validate_join_column, validate_raw_sql_fragment,
    validate_subquery_sql,
};
pub(crate) use arrays::{
    postgres_array_contained_by, postgres_array_contains, postgres_array_overlaps,
};
pub(crate) use placeholders::{inline_parameters, offset_postgres_placeholders, placeholders};

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

/// `M`'s table as a statement names it: quoted, and qualified with the model's
/// schema when it declares one.
///
/// Column references stay qualified with the bare table name, which every
/// backend resolves against a schema-qualified `FROM`.
pub(crate) fn quote_table<M: crate::model::ModelMeta>(db_type: DatabaseType) -> String {
    let mut table = String::new();
    push_quoted_table::<M>(&mut table, db_type);
    table
}

/// Append [`quote_table`]'s rendering to `out`.
pub(crate) fn push_quoted_table<M: crate::model::ModelMeta>(
    out: &mut String,
    db_type: DatabaseType,
) {
    if let Some(schema) = M::schema_name() {
        push_quoted_ident(out, db_type, schema);
        out.push('.');
    }
    push_quoted_ident(out, db_type, M::table_name());
}

/// `M`'s own columns as a projection, each qualified with `qualifier` when given.
///
/// Model reads name their columns instead of selecting `*`, so a column added
/// to the table cannot change the result shape of a statement a connection has
/// already prepared: PostgreSQL rejects that statement (`cached plan must not
/// change result type`) until the connection is recycled, and SQLite's driver
/// mis-decodes its rows.
pub(crate) fn model_columns_sql<M: crate::model::ModelMeta>(
    db_type: DatabaseType,
    qualifier: Option<&str>,
) -> String {
    let columns = M::column_names();
    let mut sql = String::with_capacity(columns.len() * 24);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        if let Some(qualifier) = qualifier {
            push_quoted_ident(&mut sql, db_type, qualifier);
            sql.push('.');
        }
        push_quoted_ident(&mut sql, db_type, column);
    }
    sql
}

/// Bind a JSON value as its JSON text, for backends that parse it server-side.
pub(crate) fn json_scalar_parameter(value: &serde_json::Value) -> Value {
    Value::String(Some(value.to_string()))
}

/// Bind a list of JSON values as the text of one JSON array.
pub(crate) fn json_array_parameter(values: &[serde_json::Value]) -> Value {
    json_scalar_parameter(&serde_json::Value::Array(values.to_vec()))
}

fn json_native_parameter(value: &serde_json::Value) -> Value {
    Value::Json(Some(Box::new(value.clone())))
}

fn json_string_contents(value: &str) -> String {
    let json = serde_json::Value::String(value.to_string()).to_string();
    json[1..json.len() - 1].to_string()
}

pub(crate) fn canonical_json_member_path(key: &str) -> String {
    format!("$.\"{}\"", json_string_contents(key))
}

/// `column_sql` as `jsonb`, which PostgreSQL's JSON operators require: a
/// `json` column has none of them. On a `jsonb` column the cast is dropped at
/// planning, so its indexes still apply.
pub(crate) fn postgres_jsonb(column_sql: &str) -> String {
    format!("({column_sql})::jsonb")
}

pub(crate) fn json_contains_bound(
    db_type: DatabaseType,
    column_sql: &str,
    value: &serde_json::Value,
) -> BoundSql {
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("{} @> $1", postgres_jsonb(column_sql)),
            vec![json_native_parameter(value)],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let (sql, values) = mysql_json::mysql_json_contains(column_sql, value);
            BoundSql::new(sql, values)
        }
        DatabaseType::SQLite => {
            let (sql, values) = sqlite_json::sqlite_json_contains(column_sql, value);
            BoundSql::new(sql, values)
        }
    }
}

pub(crate) fn json_contained_by_bound(
    db_type: DatabaseType,
    column_sql: &str,
    value: &serde_json::Value,
) -> BoundSql {
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("{} <@ $1", postgres_jsonb(column_sql)),
            vec![json_native_parameter(value)],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            let (sql, values) = mysql_json::mysql_json_contained_by(column_sql, value);
            BoundSql::new(sql, values)
        }
        DatabaseType::SQLite => {
            let (sql, values) = sqlite_json::sqlite_json_contained_by(column_sql, value);
            BoundSql::new(sql, values)
        }
    }
}

/// What a JSON existence predicate looks for.
#[derive(Clone, Copy)]
pub(crate) enum JsonExistence {
    /// A top-level member, named by a plain key.
    Key,
    /// A JSON path such as `$.user.name`.
    Path,
}

/// Render a JSON key- or path-existence test, or its negation.
///
/// PostgreSQL tests a key with `?` and a path with `@?`. MySQL/MariaDB and
/// SQLite have no key operator, so a key is tested as the one-member path that
/// names it. `None` means the path cannot be expressed on this backend.
pub(crate) fn json_exists_bound(
    db_type: DatabaseType,
    column_sql: &str,
    existence: JsonExistence,
    target: &str,
    negated: bool,
) -> Option<BoundSql> {
    let (sql, bound) = match (db_type, existence) {
        (DatabaseType::Postgres, JsonExistence::Key) => (
            format!("{} ? $1", postgres_jsonb(column_sql)),
            target.to_string(),
        ),
        (DatabaseType::Postgres, JsonExistence::Path) => (
            format!("{} @? ($1::jsonpath)", postgres_jsonb(column_sql)),
            target.to_string(),
        ),
        (DatabaseType::SQLite, JsonExistence::Key) => (
            format!("json_extract({}, ?) IS NOT NULL", column_sql),
            canonical_json_member_path(target),
        ),
        (DatabaseType::SQLite, JsonExistence::Path) => (
            format!("json_extract({}, ?) IS NOT NULL", column_sql),
            normalize_mysql_sqlite_json_path(target)?,
        ),
        (DatabaseType::MySQL | DatabaseType::MariaDB, JsonExistence::Key) => (
            format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
            canonical_json_member_path(target),
        ),
        (DatabaseType::MySQL | DatabaseType::MariaDB, JsonExistence::Path) => (
            format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
            normalize_mysql_sqlite_json_path(target)?,
        ),
    };

    let sql = if negated {
        format!("NOT ({})", sql)
    } else {
        sql
    };
    Some(BoundSql::new(sql, vec![Value::String(Some(bound))]))
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

/// The predicate a JSON path that cannot be expressed renders as: it matches
/// nothing rather than being dropped from the WHERE clause.
pub(crate) fn invalid_json_path_predicate() -> String {
    "0 = 1".to_string()
}

/// Format a trusted column/expression slot for rendering paths that intentionally
/// allow raw SQL expressions after higher-level validation.
///
/// Anything that is not a plain identifier reference is emitted verbatim, so a
/// caller must have validated it first. ORDER BY and GROUP BY no longer reach
/// here with unvalidated text: `QueryBuilder` restricts those slots to a
/// resolvable column and routes real expressions through `order_by_raw()`.
pub(crate) fn format_column_or_trusted_expression(
    db_type: DatabaseType,
    column_or_expression: &str,
) -> String {
    let trimmed = column_or_expression.trim();
    format_identifier_reference(db_type, trimmed).unwrap_or_else(|| trimmed.to_string())
}

/// Format a column identifier for the database.
///
/// This helper is intentionally strict: if the input is not a simple
/// identifier reference like `column` or `table.column`, it is quoted as a
/// single identifier instead of being passed through as raw SQL. Call
/// `format_column_or_trusted_expression()` only from rendering paths that
/// intentionally support validated raw expressions.
pub(crate) fn format_column(db_type: DatabaseType, column: &str) -> String {
    let trimmed = column.trim();
    format_identifier_reference(db_type, trimmed).unwrap_or_else(|| quote_ident(db_type, trimmed))
}

/// Cast `expr` to the backend's double-precision float type.
pub(crate) fn cast_to_float(db_type: DatabaseType, expr: &str) -> String {
    match db_type {
        DatabaseType::Postgres => format!("CAST({} AS FLOAT8)", expr),
        DatabaseType::MySQL | DatabaseType::MariaDB => format!("CAST({} AS DOUBLE)", expr),
        DatabaseType::SQLite => format!("CAST({} AS REAL)", expr),
    }
}
