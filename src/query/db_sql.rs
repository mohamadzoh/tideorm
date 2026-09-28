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
    all_elements, any_element, array_contained_by, postgres_array_contained_by,
    postgres_array_contains, postgres_array_overlaps,
};

/// A predicate that holds for no row. A condition that cannot hold (an
/// overlap with no values, a search with no word) renders as it rather than
/// leaving the WHERE clause.
pub(crate) const MATCH_NOTHING: &str = "0 = 1";

/// `SELECT <projection> FROM (<inner_sql>) AS <alias>`: a query's rows read
/// through a derived table.
pub(crate) fn select_from_derived(
    db_type: DatabaseType,
    projection: &str,
    inner_sql: &str,
    alias: &str,
) -> String {
    format!(
        "SELECT {projection} FROM ({inner_sql}) AS {}",
        quote_ident(db_type, alias)
    )
}

/// A predicate that holds for every row, such as containment of no values.
/// The mutation guard recognizes it as no restriction.
pub(crate) const MATCH_EVERYTHING: &str = "1 = 1";
pub(crate) use placeholders::{
    count_template_placeholders, inline_parameters, map_template_placeholders, placeholders,
    rebase_placeholders, render_template,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundSql {
    pub sql: String,
    pub values: Vec<Value>,
}

impl BoundSql {
    pub(crate) fn new(sql: String, values: Vec<Value>) -> Self {
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

/// The JSON path of the member `key` below `path`, the key quoted.
fn json_member_path(path: &str, key: &str) -> String {
    format!("{path}.\"{}\"", json_string_contents(key))
}

pub(crate) fn canonical_json_member_path(key: &str) -> String {
    json_member_path("$", key)
}

/// `column_sql` as `jsonb`, which PostgreSQL's JSON operators require: a
/// `json` column has none of them. On a `jsonb` column the cast is dropped at
/// planning, so its indexes still apply.
pub(crate) fn postgres_jsonb(column_sql: &str) -> String {
    format!("({column_sql})::jsonb")
}

/// Render "`column` holds the JSON document `value`" (or does not, when
/// `negated`), compared as JSON rather than as text, so key order and
/// spacing do not count and `1` equals `1.0`. PostgreSQL's `json` type has
/// no `=` at all, MySQL compared the document with a string, which never
/// matched, and SQLite, which compares JSON only as text, compares the
/// document's structure.
pub(crate) fn json_equals_bound(
    db_type: DatabaseType,
    column_sql: &str,
    value: &serde_json::Value,
    negated: bool,
) -> BoundSql {
    let operator = if negated { "<>" } else { "=" };
    let text = || vec![json_scalar_parameter(value)];
    match db_type {
        DatabaseType::Postgres => BoundSql::new(
            format!("{} {} $1", postgres_jsonb(column_sql), operator),
            vec![json_native_parameter(value)],
        ),
        DatabaseType::MySQL => BoundSql::new(
            format!("{} {} CAST(? AS JSON)", column_sql, operator),
            text(),
        ),
        // MariaDB has no `CAST(.. AS JSON)`.
        DatabaseType::MariaDB => BoundSql::new(
            format!(
                "{}JSON_EQUALS({}, ?)",
                if negated { "NOT " } else { "" },
                column_sql
            ),
            text(),
        ),
        // A NULL column matches neither the test nor its negation.
        DatabaseType::SQLite => {
            let (equal, values) = sqlite_json::sqlite_json_equals(column_sql, value);
            let sql = if negated {
                format!("({column_sql} IS NOT NULL AND NOT {equal})")
            } else {
                equal
            };
            BoundSql::new(sql, values)
        }
    }
}

/// Render "the `json_each` row `alias` holds the JSON value `value`" on
/// SQLite, compared as [`json_equals_bound`] compares documents.
pub(crate) fn sqlite_json_element_equals(alias: &str, value: &serde_json::Value) -> BoundSql {
    let (sql, values) = sqlite_json::sqlite_json_element_equals(alias, value);
    BoundSql::new(sql, values)
}

/// Which way a JSON containment test reads, as PostgreSQL's `@>` and `<@` do.
#[derive(Clone, Copy)]
pub(crate) enum JsonContainment {
    /// The column holds every key and element of the value.
    Contains,
    /// The value holds every key and element of the column.
    ContainedBy,
}

/// Render a JSON containment test of `column_sql` against `value`.
pub(crate) fn json_containment_bound(
    db_type: DatabaseType,
    column_sql: &str,
    value: &serde_json::Value,
    containment: JsonContainment,
) -> BoundSql {
    let contains = matches!(containment, JsonContainment::Contains);
    let (sql, values) = match db_type {
        DatabaseType::Postgres => (
            format!(
                "{} {} $1",
                postgres_jsonb(column_sql),
                if contains { "@>" } else { "<@" }
            ),
            vec![json_native_parameter(value)],
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB if contains => {
            mysql_json::mysql_json_contains(column_sql, value)
        }
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            mysql_json::mysql_json_contained_by(column_sql, value)
        }
        DatabaseType::SQLite if contains => sqlite_json::sqlite_json_contains(column_sql, value),
        DatabaseType::SQLite => sqlite_json::sqlite_json_contained_by(column_sql, value),
    };
    BoundSql::new(sql, values)
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
///
/// On SQLite, `json_type` finds a member that holds JSON `null`, which
/// `json_extract` reports as SQL `NULL`, and the `CASE` leaves a `NULL`
/// column unknown, as the other backends do, so that the negation does not
/// match it either.
pub(crate) fn json_exists_bound(
    db_type: DatabaseType,
    column_sql: &str,
    existence: JsonExistence,
    target: &str,
    negated: bool,
) -> Option<BoundSql> {
    // PostgreSQL reads a key or a JSONPath as written; the others take the
    // path of the one member a key names.
    let bound = match (db_type, existence) {
        (DatabaseType::Postgres, _) => target.to_string(),
        (_, JsonExistence::Key) => canonical_json_member_path(target),
        (_, JsonExistence::Path) => json_path_text(&parse_json_path(target)?),
    };
    let sql = match (db_type, existence) {
        (DatabaseType::Postgres, JsonExistence::Key) => {
            format!("{} ? $1", postgres_jsonb(column_sql))
        }
        (DatabaseType::Postgres, JsonExistence::Path) => {
            format!("{} @? ($1::jsonpath)", postgres_jsonb(column_sql))
        }
        (DatabaseType::SQLite, _) => format!(
            "CASE WHEN {column_sql} IS NOT NULL THEN json_type({column_sql}, ?) IS NOT NULL END"
        ),
        (DatabaseType::MySQL | DatabaseType::MariaDB, _) => {
            format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql)
        }
    };

    let sql = if negated {
        format!("NOT ({})", sql)
    } else {
        sql
    };
    Some(BoundSql::new(sql, vec![Value::String(Some(bound))]))
}

/// One step of a JSON path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonPathStep {
    /// The member a key names.
    Member(String),
    /// The array element at an index.
    Element(u64),
}

/// The steps of a JSON path written `$` followed by `.key`, `."quoted key"`,
/// `['quoted key']` or `[index]` steps; `None` for anything else, such as a
/// wildcard or an unquoted key that is not a plain identifier.
pub(crate) fn parse_json_path(path: &str) -> Option<Vec<JsonPathStep>> {
    let chars: Vec<char> = path.chars().collect();
    if chars.first().copied() != Some('$') {
        return None;
    }

    let mut index = 1;
    let mut steps = Vec::new();
    while index < chars.len() {
        let step = match chars[index] {
            '.' => {
                index += 1;
                match chars.get(index) {
                    Some('"' | '\'') => {
                        JsonPathStep::Member(parse_quoted_json_path_segment(&chars, &mut index)?)
                    }
                    Some(_) => {
                        let start = index;
                        while index < chars.len() && chars[index] != '.' && chars[index] != '[' {
                            index += 1;
                        }
                        let segment: String = chars[start..index].iter().collect();
                        if !is_safe_identifier_segment(&segment) {
                            return None;
                        }
                        JsonPathStep::Member(segment)
                    }
                    None => return None,
                }
            }
            '[' => {
                index += 1;
                let step = match chars.get(index) {
                    Some('"' | '\'') => {
                        JsonPathStep::Member(parse_quoted_json_path_segment(&chars, &mut index)?)
                    }
                    Some(ch) if ch.is_ascii_digit() => {
                        let start = index;
                        while index < chars.len() && chars[index].is_ascii_digit() {
                            index += 1;
                        }
                        let digits: String = chars[start..index].iter().collect();
                        JsonPathStep::Element(digits.parse().ok()?)
                    }
                    _ => return None,
                };
                if chars.get(index) != Some(&']') {
                    return None;
                }
                index += 1;
                step
            }
            _ => return None,
        };
        steps.push(step);
    }

    Some(steps)
}

/// A JSON path as MySQL, MariaDB and SQLite read it, every key quoted.
pub(crate) fn json_path_text(steps: &[JsonPathStep]) -> String {
    steps
        .iter()
        .fold(String::from("$"), |path, step| match step {
            JsonPathStep::Member(key) => json_member_path(&path, key),
            JsonPathStep::Element(index) => format!("{path}[{index}]"),
        })
}

/// A JSON path as the `text[]` literal PostgreSQL's `jsonb_set` takes.
pub(crate) fn postgres_json_path_array(steps: &[JsonPathStep]) -> String {
    let elements: Vec<String> = steps
        .iter()
        .map(|step| {
            let text = match step {
                JsonPathStep::Member(key) => key.clone(),
                JsonPathStep::Element(index) => index.to_string(),
            };
            format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
        })
        .collect();
    format!("{{{}}}", elements.join(","))
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

/// Append a query's ` LIMIT n OFFSET ?`: the limit written into the SQL, where
/// SQLite 3.50+ would recompile a statement with a bound LIMIT on every run,
/// and the offset bound, so paging reuses one statement. MySQL, MariaDB and
/// SQLite have no bare `OFFSET`, so an offset alone gets the dialect's
/// open-ended limit.
pub(crate) fn append_limit_offset(
    sql: &mut String,
    db_type: DatabaseType,
    limit: Option<u64>,
    offset: Option<u64>,
    params: &mut Vec<Value>,
) {
    match (limit, offset) {
        (Some(limit), _) => sql.push_str(&format!(" LIMIT {limit}")),
        (None, Some(_)) => match db_type {
            DatabaseType::Postgres => {}
            DatabaseType::SQLite => sql.push_str(" LIMIT -1"),
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                sql.push_str(&format!(" LIMIT {}", u64::MAX))
            }
        },
        (None, None) => {}
    }
    if let Some(offset) = offset {
        let offset = match i64::try_from(offset) {
            Ok(offset) => crate::internal::push_param(db_type, params, Value::BigInt(Some(offset))),
            Err(_) => offset.to_string(),
        };
        sql.push_str(&format!(" OFFSET {offset}"));
    }
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
