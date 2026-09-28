//! Array predicates: each backend tests its elements its own way, but every
//! one assembles the same two shapes from those tests.
//!
//! PostgreSQL's are written without the `ARRAY[..]` constructor. sea-query's
//! fragment tokenizer treats `[` as a string delimiter that ends at `]`, so a
//! placeholder written between the brackets is skipped by
//! `Expr::cust_with_values` and its value silently never binds.
//! `x = ANY(column)` is the bracket-free equivalent and additionally lets
//! PostgreSQL infer each parameter's type from the column's element type.
//! `operands` are the bound placeholders (`$1`, `$2`, ..) of the fragment.

use super::{MATCH_EVERYTHING, MATCH_NOTHING};

/// Alias given to the `unnest(..)` derived table in the contained-by rendering,
/// together with the name of its single column.
const POSTGRES_ARRAY_ELEMENT_ALIAS: &str = "tideorm_array_element";

/// Render `column @> ARRAY[..]`.
pub(crate) fn postgres_array_contains(column: &str, operands: &[String]) -> String {
    all_elements(&postgres_element_checks(column, operands))
}

/// Render `column && ARRAY[..]` (overlap).
pub(crate) fn postgres_array_overlaps(column: &str, operands: &[String]) -> String {
    any_element(&postgres_element_checks(column, operands))
}

fn postgres_element_checks(column: &str, operands: &[String]) -> Vec<String> {
    operands
        .iter()
        .map(|operand| format!("{operand} = ANY({column})"))
        .collect()
}

/// Every one of the element `checks` holds: what containment of a list
/// tests. A list of no elements is vacuously contained.
pub(crate) fn all_elements(checks: &[String]) -> String {
    joined(checks, " AND ", MATCH_EVERYTHING)
}

/// One of the element `checks` holds: what an overlap with a list tests.
/// Nothing overlaps a list of no elements.
pub(crate) fn any_element(checks: &[String]) -> String {
    joined(checks, " OR ", MATCH_NOTHING)
}

fn joined(checks: &[String], combine: &str, empty: &str) -> String {
    if checks.is_empty() {
        return empty.to_string();
    }
    format!("({})", checks.join(combine))
}

/// Render `column <@ ARRAY[..]` (contained by).
///
/// `operands` are the placeholders of the list's non-NULL values, and
/// `null_allowed` says whether the list also holds a NULL.
///
/// An empty list leaves only the empty array contained, and a NULL element is
/// contained only when the list holds a NULL. A NULL *element* makes
/// `element NOT IN (..)` evaluate to NULL rather than TRUE, so the offending
/// row would not be counted; `element IS NULL OR ..` counts it instead. A NULL
/// in the list is left out of `NOT IN` for the same reason: it would make
/// every element look contained.
pub(crate) fn postgres_array_contained_by(
    column: &str,
    operands: &[String],
    null_allowed: bool,
) -> String {
    let alias = POSTGRES_ARRAY_ELEMENT_ALIAS;
    let element = format!("{alias}.element");
    let offending = match (operands.is_empty(), null_allowed) {
        (true, false) => None,
        (true, true) => Some(format!("{element} IS NOT NULL")),
        (false, false) => Some(format!(
            "{element} IS NULL OR {element} NOT IN ({})",
            operands.join(", ")
        )),
        (false, true) => Some(format!("{element} NOT IN ({})", operands.join(", "))),
    };
    array_contained_by(
        column,
        &format!("unnest({column}) AS {alias}(element)"),
        offending.as_deref(),
    )
}

/// Render "every element of `column` is in the list" over `source`, a table of
/// its elements, of which `offending` selects those the list does not hold
/// (`None`: every element offends).
///
/// A NULL column has no elements, so a bare `NOT EXISTS` is TRUE for it —
/// where `NULL <@ ARRAY[..]` is NULL and matches nothing. The explicit
/// `IS NOT NULL` restores that. Without it a NULL array column silently
/// matches, which widens a SELECT and, on a mutation terminal, widens the set
/// of rows written or deleted.
pub(crate) fn array_contained_by(column: &str, source: &str, offending: Option<&str>) -> String {
    match offending {
        None => format!("({column} IS NOT NULL AND NOT EXISTS (SELECT 1 FROM {source}))"),
        Some(offending) => format!(
            "({column} IS NOT NULL AND NOT EXISTS (SELECT 1 FROM {source} WHERE {offending}))"
        ),
    }
}
