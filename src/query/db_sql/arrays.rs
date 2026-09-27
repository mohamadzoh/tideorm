//! PostgreSQL array predicates written without the `ARRAY[..]` constructor.
//!
//! sea-query's fragment tokenizer treats `[` as a string delimiter that ends at
//! `]`, so a placeholder written between the brackets is skipped by
//! `Expr::cust_with_values` and its value silently never binds. `x = ANY(column)`
//! is the bracket-free equivalent and additionally lets PostgreSQL infer each
//! parameter's type from the column's element type. `operands` are the bound
//! placeholders (`$1`, `$2`, ..) of the fragment.

/// Alias given to the `unnest(..)` derived table in the contained-by rendering,
/// together with the name of its single column.
const POSTGRES_ARRAY_ELEMENT_ALIAS: &str = "tideorm_array_element";

/// Render `column @> ARRAY[..]`. An empty operand list is vacuously contained,
/// so it renders as true.
pub(crate) fn postgres_array_contains(column: &str, operands: &[String]) -> String {
    postgres_array_element_match(column, operands, " AND ", "1 = 1")
}

/// Render `column && ARRAY[..]` (overlap). An empty operand list cannot
/// overlap with anything, so it renders as false.
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

/// Render `column <@ ARRAY[..]` (contained by).
///
/// `operands` are the placeholders of the list's non-NULL values, and
/// `null_allowed` says whether the list also holds a NULL.
pub(crate) fn postgres_array_contained_by(
    column: &str,
    operands: &[String],
    null_allowed: bool,
) -> String {
    let alias = POSTGRES_ARRAY_ELEMENT_ALIAS;
    let source = format!("unnest({}) AS {}(element)", column, alias);
    array_contained_by(
        column,
        &source,
        &format!("{alias}.element"),
        operands,
        null_allowed,
    )
}

/// Render "every element of `column` is in the list" over `source`, a table of
/// its elements whose values `element` names.
///
/// An empty list leaves only the empty array contained, and a NULL element is
/// contained only when the list holds a NULL. Both NULL guards are
/// load-bearing, because `NOT EXISTS` inverts what `<@` does with unknowns:
///
/// - a NULL column has no elements, so a bare `NOT EXISTS` is TRUE for it —
///   where `NULL <@ ARRAY[..]` is NULL and matches nothing. The explicit
///   `IS NOT NULL` restores that.
/// - a NULL *element* makes `element NOT IN (..)` evaluate to NULL rather than
///   TRUE, so the offending row is not counted and `NOT EXISTS` again reports
///   containment. `element IS NULL OR ..` counts it instead. A NULL in the list
///   is left out of `NOT IN` for the same reason: it would make every element
///   look contained.
///
/// Without them a NULL array column silently matches, which widens a SELECT and,
/// on a mutation terminal, widens the set of rows written or deleted.
fn array_contained_by(
    column: &str,
    source: &str,
    element: &str,
    operands: &[String],
    null_allowed: bool,
) -> String {
    let offending = match (operands.is_empty(), null_allowed) {
        (true, false) => None,
        (true, true) => Some(format!("{element} IS NOT NULL")),
        (false, false) => Some(format!(
            "{element} IS NULL OR {element} NOT IN ({})",
            operands.join(", ")
        )),
        (false, true) => Some(format!("{element} NOT IN ({})", operands.join(", "))),
    };
    match offending {
        None => format!("({column} IS NOT NULL AND NOT EXISTS (SELECT 1 FROM {source}))"),
        Some(offending) => format!(
            "({column} IS NOT NULL AND NOT EXISTS (SELECT 1 FROM {source} WHERE {offending}))"
        ),
    }
}
