//! The operand of a filter: typed values, each bound as its own type.
//!
//! A Rust value keeps its type from the call to the statement: a `Uuid`, a
//! timestamp or a decimal binds as one, an integer at its width, a `None` as
//! SQL NULL. A `serde_json::Value` is the exception, since JSON has no SQL
//! type: a JSON column compares it as a document, and any other column reads
//! it as a value of the column's own type.

use super::{Operator, WhereCondition};
use crate::internal::{Value, value_document as document_of};

/// Value for a where condition
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConditionValue {
    /// One value. A JSON value (`Value::Json`) is a document to a JSON column
    /// and a value of the column's type to any other.
    Single(Value),
    /// The values of an `IN` list or an array operator. A NULL member is SQL
    /// NULL; a JSON `null` document is a document to a JSON column and SQL
    /// NULL to any other.
    List(Vec<Value>),
    /// The low and high bounds of a `BETWEEN`.
    Range(Value, Value),
    /// No operand: `IS NULL` and `IS NOT NULL`.
    None,
    /// Trusted SQL written in as given: a raw condition's text, or a column
    /// compared with another.
    RawExpr(String),
    /// A query the condition embeds (`where_in_subquery`, `where_exists`,
    /// `where_has`, the `has_related` family), rendered with the statement for
    /// the backend it runs on, its values bound.
    Subquery(crate::query::Subquery),
    /// A caller's raw fragment written with a `?` per value, outside quoted
    /// literals, on every backend; the `?`s become the rendering backend's own
    /// markers when the statement is built, so the fragment follows the query
    /// to whichever backend runs it.
    RawTemplate {
        /// The fragment.
        sql: String,
        /// The values, in placeholder order.
        values: Vec<Value>,
    },
    /// Another column of the same row, which the condition's column is
    /// compared with (`where_column_gt("updated_at", "created_at")`).
    Column(String),
    /// A value no SQL comparison can take, such as a NaN float, with the
    /// reason the query fails with when it runs.
    Invalid(String),
}

/// The value as JSON — a list as an array, a range as `low..high` — or the
/// SQL or column it carries.
impl std::fmt::Display for ConditionValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Single(value) => write!(f, "{}", document_of(value)),
            Self::List(values) => write!(
                f,
                "{}",
                serde_json::Value::Array(values.iter().map(document_of).collect())
            ),
            Self::Range(low, high) => write!(f, "{}..{}", document_of(low), document_of(high)),
            Self::None => f.write_str("NULL"),
            Self::RawExpr(sql) | Self::RawTemplate { sql, .. } => f.write_str(sql),
            Self::Subquery(subquery) => write!(f, "{}", subquery),
            Self::Column(column) => f.write_str(column),
            Self::Invalid(reason) => write!(f, "<invalid: {}>", reason),
        }
    }
}

/// Why a value holding a NaN or an infinite float is refused.
const NON_FINITE: &str = "a filter value holds a NaN or infinite float, which SQL cannot compare";

impl ConditionValue {
    /// One filter value; [`Invalid`](Self::Invalid) for one SQL cannot compare.
    ///
    /// SQL NULL, a `None` of any type, is [`None`](Self::None). A JSON `null`
    /// document (`serde_json::Value::Null`) stays a value, which a JSON column
    /// compares as the `null` document.
    pub(crate) fn single(value: impl Into<Value>) -> Self {
        match comparable_value(value) {
            Ok(value) if is_sql_null(&value) => Self::None,
            Ok(value) => Self::Single(value),
            Err(reason) => Self::Invalid(reason),
        }
    }

    /// A list of filter values, refused as a whole when one cannot be compared.
    pub(crate) fn list<V: Into<Value>>(values: impl IntoIterator<Item = V>) -> Self {
        let values: Vec<Value> = values.into_iter().map(Into::into).collect();
        if values.iter().any(non_finite) {
            Self::Invalid(NON_FINITE.to_string())
        } else {
            Self::List(values)
        }
    }

    /// The bounds of a range.
    pub(crate) fn range(low: impl Into<Value>, high: impl Into<Value>) -> Self {
        let (low, high) = (low.into(), high.into());
        if non_finite(&low) || non_finite(&high) {
            Self::Invalid(NON_FINITE.to_string())
        } else {
            Self::Range(low, high)
        }
    }

    /// A JSON document, for a containment test. A `None` is SQL NULL, which
    /// no document contains.
    pub(crate) fn document(value: impl serde::Serialize) -> Self {
        match super::finite::checked_document(value) {
            Ok((_, true)) => Self::None,
            Ok((document, false)) => Self::Single(Value::Json(Some(Box::new(document)))),
            Err(reason) => Self::Invalid(reason),
        }
    }

    /// One text value: a `LIKE` pattern, a JSON key or path.
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self::Single(Value::String(Some(text.into())))
    }

    /// A JSON path for an existence test: `$` and its steps, or plain steps
    /// read from the root (`a.b[0]` is `$.a.b[0]`). Anything else is kept as
    /// written, for PostgreSQL to read as a JSONPath.
    pub(crate) fn json_path(path: &str) -> Self {
        let rooted = match path.chars().next() {
            Some('$') | None => None,
            Some('[') => Some(format!("${path}")),
            Some(_) => Some(format!("$.{path}")),
        };
        match rooted.filter(|rooted| crate::query::db_sql::parse_json_path(rooted).is_some()) {
            Some(rooted) => Self::text(rooted),
            None => Self::text(path),
        }
    }
}

/// Whether `value` is SQL NULL, of whatever type. A JSON `null` document is
/// a value.
pub(crate) fn is_sql_null(value: &Value) -> bool {
    *value == value.as_null()
}

/// Whether `value` is the JSON `null` document.
pub(crate) fn is_json_null(value: &Value) -> bool {
    matches!(value, Value::Json(Some(json)) if json.is_null())
}

/// `value`, or why SQL cannot compare it: it holds a NaN or an infinite float.
pub(crate) fn comparable_value(value: impl Into<Value>) -> Result<Value, String> {
    let value = value.into();
    if non_finite(&value) {
        Err(NON_FINITE.to_string())
    } else {
        Ok(value)
    }
}

/// Whether `value` holds a NaN or an infinite float.
fn non_finite(value: &Value) -> bool {
    match value {
        Value::Float(Some(float)) => !float.is_finite(),
        Value::Double(Some(float)) => !float.is_finite(),
        Value::Array(_, Some(items)) => items.iter().any(non_finite),
        _ => false,
    }
}

/// True when a declared condition cannot exclude any row.
///
/// These are the operand shapes that render constant-true: an empty candidate
/// set for a *negative* membership test, and an empty "contains all of these"
/// array test. A caller reaches them by accident — a filter list that came back
/// empty from a form or an upstream query — and the resulting predicate silently
/// widens a targeted mutation into a whole-table one. A negative list holding
/// only NULLs counts too: it renders `col IS NOT NULL`, which keeps every row of
/// a `NOT NULL` column, as does `where_not(col, None)`, which renders the same test.
///
/// Their positive duals (`IN ()`, `&& ()`) render constant-*false*
/// and are deliberately absent: a mutation that matches nothing is safe.
///
/// The check stays structural even though empty membership renders literal
/// constants: NULL-only lists, wildcard patterns, JSON and array predicates can
/// also be vacuous. A soft-delete scope adds its own restrictive conjunct, so
/// checking the final SQL alone cannot authorize the caller's filter.
///
/// `is_json` tells the columns holding JSON, where a JSON `null` is the `null`
/// document rather than SQL NULL, so a test against it keeps some rows.
pub(crate) fn condition_is_vacuous(
    condition: &WhereCondition,
    is_json: impl Fn(&str) -> bool,
) -> bool {
    match (&condition.operator, &condition.value) {
        (Operator::NotIn, ConditionValue::List(values)) => {
            let documents = values.iter().any(is_json_null) && is_json(&condition.column);
            values
                .iter()
                .all(|value| is_sql_null(value) || (!documents && is_json_null(value)))
        }
        // `where_not(col, None)` renders the same `col IS NOT NULL`.
        (Operator::NotEq, ConditionValue::None) => true,
        (Operator::NotEq, ConditionValue::Single(value)) if is_json_null(value) => {
            !is_json(&condition.column)
        }
        (Operator::ArrayContains, ConditionValue::List(values)) => values.is_empty(),
        // A pattern of `%` alone matches every value, as an empty `contains`,
        // `starts_with` or `ends_with` does.
        (Operator::Like, ConditionValue::Single(Value::String(Some(pattern)))) => {
            !pattern.is_empty() && pattern.chars().all(|character| character == '%')
        }
        (
            Operator::Contains | Operator::StartsWith | Operator::EndsWith,
            ConditionValue::Single(Value::String(Some(literal))),
        ) => literal.is_empty(),
        // Every object document contains `{}`, and every array `[]`.
        (Operator::JsonContains, ConditionValue::Single(Value::Json(Some(document)))) => {
            document.as_object().is_some_and(serde_json::Map::is_empty)
                || document.as_array().is_some_and(Vec::is_empty)
        }
        _ => false,
    }
}
