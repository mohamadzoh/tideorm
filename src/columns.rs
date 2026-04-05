//! Strongly-Typed Columns
//!
//! Typed columns let query builders refer to generated column constants instead
//! of raw strings.
//!
//! The practical benefit is earlier failure: misspelled local model columns are
//! caught by the compiler or by model-column validation instead of surfacing as
//! a runtime SQL error later.
//!
//! Use these helpers when raw string column names become hard to audit or easy
//! to misspell in larger query builders.

use std::marker::PhantomData;

mod impls;

// =============================================================================
// TYPED COLUMN
// =============================================================================

/// Trait for types that can be used as column names in queries.
///
/// This allows both string literals and typed `Column<T>` to be used
/// interchangeably in query methods like `where_eq`.
pub trait IntoColumnName {
    /// Get the column name as a string
    fn column_name(&self) -> &str;
}

impl IntoColumnName for &str {
    fn column_name(&self) -> &str {
        self
    }
}

impl IntoColumnName for String {
    fn column_name(&self) -> &str {
        self.as_str()
    }
}

impl IntoColumnName for &String {
    fn column_name(&self) -> &str {
        self.as_str()
    }
}

impl<T> IntoColumnName for Column<T> {
    fn column_name(&self) -> &str {
        self.name
    }
}

/// A strongly-typed column reference
///
/// This provides compile-time type safety for column operations.
/// The type parameter `T` represents the Rust type of the column.
#[derive(Debug, Clone, Copy)]
pub struct Column<T> {
    name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T> Column<T> {
    /// Create a new typed column reference
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _phantom: PhantomData,
        }
    }

    /// Get the column name
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

// =============================================================================
// COLUMN CONDITIONS
// =============================================================================

/// A type-safe column condition for WHERE clauses
#[derive(Debug, Clone)]
pub struct ColumnCondition {
    /// The column name
    pub column: String,
    /// The operator
    pub operator: ColumnOperator,
    /// The value (as JSON for flexibility)
    pub value: serde_json::Value,
}

/// Operators for column conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnOperator {
    /// Equal to (=)
    Eq,
    /// Not equal to (<>)
    NotEq,
    /// Greater than (>)
    Gt,
    /// Greater than or equal (>=)
    Gte,
    /// Less than (<)
    Lt,
    /// Less than or equal (<=)
    Lte,
    /// LIKE pattern match
    Like,
    /// LIKE pattern match using an escaped literal pattern
    LikeEscaped,
    /// NOT LIKE pattern match
    NotLike,
    /// IN list
    In,
    /// NOT IN list
    NotIn,
    /// IS NULL
    IsNull,
    /// IS NOT NULL
    IsNotNull,
    /// BETWEEN range
    Between,
}

impl ColumnOperator {
    /// Convert to SQL operator string
    pub fn to_sql(&self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Like => "LIKE",
            Self::LikeEscaped => "LIKE",
            Self::NotLike => "NOT LIKE",
            Self::In => "IN",
            Self::NotIn => "NOT IN",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
            Self::Between => "BETWEEN",
        }
    }
}

pub(crate) fn escape_like_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

// =============================================================================
// COLUMN EXPRESSION TRAITS
// =============================================================================

/// Trait for types that can be compared for equality
pub trait ColumnEq<T> {
    /// Create an equals condition
    fn eq(self, value: T) -> ColumnCondition;
    /// Create a not equals condition
    fn ne(self, value: T) -> ColumnCondition;
}

/// Trait for types that support ordering comparisons
pub trait ColumnOrd<T>: ColumnEq<T> {
    /// Create a greater than condition
    fn gt(self, value: T) -> ColumnCondition;
    /// Create a greater than or equal condition
    fn gte(self, value: T) -> ColumnCondition;
    /// Create a less than condition
    fn lt(self, value: T) -> ColumnCondition;
    /// Create a less than or equal condition
    fn lte(self, value: T) -> ColumnCondition;
    /// Create a between condition
    fn between(self, low: T, high: T) -> ColumnCondition;
}

/// Trait for string-like types that support LIKE
pub trait ColumnLike {
    /// Create a LIKE pattern condition
    fn like(self, pattern: &str) -> ColumnCondition;
    /// Create a NOT LIKE pattern condition
    fn not_like(self, pattern: &str) -> ColumnCondition;
    /// Create a LIKE '%value%' condition
    fn contains(self, substr: &str) -> ColumnCondition;
    /// Create a LIKE 'value%' condition
    fn starts_with(self, prefix: &str) -> ColumnCondition;
    /// Create a LIKE '%value' condition
    fn ends_with(self, suffix: &str) -> ColumnCondition;
}

/// Trait for nullable columns
#[allow(clippy::wrong_self_convention)]
pub trait ColumnNullable {
    /// Create an IS NULL condition
    fn is_null(self) -> ColumnCondition;
    /// Create an IS NOT NULL condition
    fn is_not_null(self) -> ColumnCondition;
}

/// Trait for types that support IN clauses
#[allow(clippy::wrong_self_convention)]
pub trait ColumnIn<T> {
    /// Create an IN list condition
    fn is_in(self, values: Vec<T>) -> ColumnCondition;
    /// Create a NOT IN list condition
    fn not_in(self, values: Vec<T>) -> ColumnCondition;
}

#[cfg(test)]
#[path = "../tests/unit/columns_tests.rs"]
mod tests;
