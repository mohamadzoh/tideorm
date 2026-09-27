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

/// Trait for types that can be used as column names in queries.
///
/// This allows both string literals and typed `Column<T>` to be used
/// interchangeably in query methods like `where_eq`.
pub trait IntoColumnName {
    /// Get the column name as a string
    fn column_name(&self) -> &str;

    /// The table the column belongs to, when it names one: a model's typed
    /// column does, a string does not.
    fn column_table(&self) -> Option<&str> {
        None
    }
}

/// How a query of the model whose table is `own_table` refers to `column`:
/// by name, qualified with the column's own table when that is another
/// model's, so `User::columns.id` in a `Post` query means `users.id` rather
/// than `posts.id`. With no `own_table`, a typed column is always qualified.
pub(crate) fn column_reference(column: &impl IntoColumnName, own_table: Option<&str>) -> String {
    match column.column_table() {
        Some(table) if Some(table) != own_table => {
            format!("{}.{}", table, column.column_name())
        }
        _ => column.column_name().to_string(),
    }
}

/// The builders whose `where_*` methods [`column_reference`] qualifies for.
pub(crate) trait ConditionOwner {
    /// The table of the builder's model, if it has one.
    fn own_table() -> Option<&'static str>;
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

    fn column_table(&self) -> Option<&str> {
        self.table
    }
}

/// A strongly-typed column reference
///
/// This provides compile-time type safety for column operations.
/// The type parameter `T` represents the Rust type of the column.
#[derive(Debug, Clone, Copy)]
pub struct Column<T> {
    table: Option<&'static str>,
    name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T> Column<T> {
    /// Create a new typed column reference
    pub const fn new(name: &'static str) -> Self {
        Self {
            table: None,
            name,
            _phantom: PhantomData,
        }
    }

    /// A column of `table`, as a model's generated `columns` name theirs: a
    /// query of another model refers to it as `table.name`.
    pub const fn of(table: &'static str, name: &'static str) -> Self {
        Self {
            table: Some(table),
            name,
            _phantom: PhantomData,
        }
    }

    /// Get the column name
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The table the column belongs to, for a model's column.
    pub const fn table(&self) -> Option<&'static str> {
        self.table
    }
}

/// A type-safe column condition for WHERE clauses, applied with
/// [`QueryBuilder::where_col`](crate::query::QueryBuilder::where_col).
#[derive(Debug, Clone)]
pub struct ColumnCondition {
    /// The column name
    pub column: String,
    /// The comparison to apply
    pub operator: crate::query::Operator,
    /// The value compared with: one value, a list for `In`/`NotIn`, a range
    /// for `Between`, and none for the NULL checks.
    pub value: crate::query::ConditionValue,
}

/// The escape character used by every generated `LIKE ... ESCAPE` clause.
///
/// Deliberately **not** a backslash. A backslash costs us on three fronts:
/// SQLite string literals do not process it (so `'\\'` is a two-character
/// escape argument SQLite rejects), MySQL's handling flips with
/// `NO_BACKSLASH_ESCAPES`, and sea-query's tokenizer treats it as an escape
/// character when it re-scans a `cust_with_values` fragment — an `ESCAPE '\'`
/// clause leaves the literal unterminated and swallows every later placeholder.
/// `!` is a single character in all three dialects, is not special inside a LIKE
/// pattern, and keeps generated SQL backslash-free.
pub(crate) const LIKE_ESCAPE_CHAR: char = '!';

/// The full ` ESCAPE '<char>'` suffix appended to generated LIKE comparisons.
pub(crate) const LIKE_ESCAPE_CLAUSE: &str = " ESCAPE '!'";

pub(crate) fn escape_like_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch == LIKE_ESCAPE_CHAR || ch == '%' || ch == '_' {
            escaped.push(LIKE_ESCAPE_CHAR);
        }
        escaped.push(ch);
    }
    escaped
}

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
    /// Create an IN list condition from any list: a `Vec`, an array, a set
    fn is_in(self, values: impl IntoIterator<Item = T>) -> ColumnCondition;
    /// Create a NOT IN list condition
    fn not_in(self, values: impl IntoIterator<Item = T>) -> ColumnCondition;
}

#[cfg(test)]
#[path = "../tests/unit/columns_tests.rs"]
mod tests;
