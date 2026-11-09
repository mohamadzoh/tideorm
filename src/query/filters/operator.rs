//! The comparisons a condition makes, and how conditions combine.

/// Comparison operators for where clauses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operator {
    /// `=`
    Eq,
    /// `<>`
    NotEq,
    /// `>`
    Gt,
    /// `>=`
    Gte,
    /// `<`
    Lt,
    /// `<=`
    Lte,
    /// `LIKE`, the pattern's `%` and `_` read as wildcards.
    Like,
    /// The text holds the value literally (`contains`).
    Contains,
    /// The text starts with the value literally (`starts_with`).
    StartsWith,
    /// The text ends with the value literally (`ends_with`).
    EndsWith,
    /// `NOT LIKE`
    NotLike,
    /// `IN (..)`
    In,
    /// `NOT IN (..)`
    NotIn,
    /// `IS NULL`
    IsNull,
    /// `IS NOT NULL`
    IsNotNull,
    /// `BETWEEN low AND high`
    Between,
    /// `NOT BETWEEN low AND high`
    NotBetween,
    /// The JSON column contains the document (PostgreSQL's `@>`).
    JsonContains,
    /// The JSON column is contained by the document (PostgreSQL's `<@`).
    JsonContainedBy,
    /// The JSON object has the key.
    JsonKeyExists,
    /// The JSON object lacks the key.
    JsonKeyNotExists,
    /// The JSON document has a member at the path.
    JsonPathExists,
    /// The JSON document has no member at the path.
    JsonPathNotExists,
    /// The array column holds every value (PostgreSQL's `@>`).
    ArrayContains,
    /// Every element of the array column is among the values (`<@`).
    ArrayContainedBy,
    /// The array column shares a value with the list (`&&`).
    ArrayOverlaps,
    /// A raw SQL condition, its text in the value.
    Raw,
}

/// Logical operator for combining conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalOp {
    /// Every member holds.
    And,
    /// Some member holds.
    Or,
}

impl LogicalOp {
    /// The SQL keyword: `AND` or `OR`.
    pub fn as_sql(&self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
    }
}
