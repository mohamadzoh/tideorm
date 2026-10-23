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

//! # Typed filter values
//!
//! ```
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool, name: String }
//! User::query().where_eq(User::ACTIVE, true).where_eq(User::NAME, "Ada").validate()?;
//! # Ok::<(), tideorm::Error>(())
//! ```
//!
//! A misspelled constant and a value of the wrong type are compile errors:
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! User::query().where_eq(User::ACTVIE, true);
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! User::query().where_eq(User::ACTIVE, "yes");
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! User::query().where_gt(User::ACTIVE, "yes");
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! User::query().where_in(User::ACTIVE, ["yes"]);
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! User::query().where_between(User::ACTIVE, "no", "yes");
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! User::query().begin_or().or_where_eq(User::ACTIVE, "yes");
//! ```
//!

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

    /// The declared schema of a typed column, when one was specified.
    fn column_schema(&self) -> Option<&str> {
        None
    }
}

/// A column that accepts `V` as a filter value.
///
/// A generated `Column<T>` accepts a `T` or a `&T`, so a borrowed list works
/// in `where_in`. A nullable `Column<Option<T>>` accepts those as well as an
/// `Option<T>`. Text columns accept `&str`, and integer, float and decimal
/// columns accept narrower numbers that convert losslessly. String column
/// names retain their dynamic, serializable values.
#[diagnostic::on_unimplemented(
    message = "a `{Self}` filter does not accept a `{V}` value",
    label = "expected the column's Rust type",
    note = "typed columns take their field's type or a reference to it, `&str` for text, or a losslessly widened number; filter by the column's name to pass any serializable value"
)]
pub trait ColumnValue<V>: IntoColumnName {
    /// The value serialized into the query parameter.
    type Value: serde::Serialize;
    /// Convert a filter value to the column's Rust type.
    fn convert_value(value: V) -> Self::Value;
}

// Concrete impls rather than one `V: Into<T>` blanket: the blanket cannot
// coexist with `&str` for `Option<String>` (coherence), and it made an
// unsuffixed literal fall back to `i32` for `u64` and `i16` columns.
macro_rules! identity_column_value {
    ($([$($generics:tt)*] $value:ty => $column:ty),* $(,)?) => {$(
        impl<$($generics)*> ColumnValue<$value> for Column<$column> {
            type Value = $value;
            fn convert_value(value: $value) -> $value { value }
        }
    )*};
}
identity_column_value!(
    [T: serde::Serialize] T => T,
    [T: serde::Serialize] T => Option<T>,
    ['a, T: serde::Serialize] &'a T => T,
    ['a, T: serde::Serialize] &'a T => Option<T>,
    ['a] &'a str => String,
    ['a] &'a str => Option<String>,
);

// Only into types an unsuffixed literal falls back to (`i32`, `f64`) or reaches
// from one: a widening into `i16` would make `where_eq(column, 5)` ambiguous.
macro_rules! widened_column_value {
    ($($column:ty => $($value:ty),+;)*) => {$($(
        impl ColumnValue<$value> for Column<$column> {
            type Value = $column;
            fn convert_value(value: $value) -> $column { value.into() }
        }
        impl ColumnValue<$value> for Column<Option<$column>> {
            type Value = $column;
            fn convert_value(value: $value) -> $column { value.into() }
        }
    )+)*};
}
widened_column_value!(
    i64 => i8, i16, i32, u8, u16, u32;
    i32 => i8, i16, u8, u16;
    f64 => f32, i8, i16, i32, u8, u16, u32;
    rust_decimal::Decimal => i8, i16, i32, i64, u8, u16, u32, u64;
);

macro_rules! dynamic_column_value {
    ($($ty:ty),*) => {$(
        impl<V: serde::Serialize> ColumnValue<V> for $ty {
            type Value = V;
            fn convert_value(value: V) -> V { value }
        }
    )*};
}
dynamic_column_value!(&str, String, &String);

/// How a query of the model whose table is `own_table` refers to `column`:
/// by name, qualified with the column's own table when that is another
/// model's, so `User::columns.id` in a `Post` query means `users.id` rather
/// than `posts.id`. With no `own_table`, a typed column is always qualified.
pub(crate) fn column_reference(
    column: &impl IntoColumnName,
    own_table: Option<(&str, Option<&str>)>,
) -> String {
    match column.column_table() {
        Some(table) if Some((table, column.column_schema())) != own_table => {
            match column.column_schema() {
                Some(schema) => format!("{schema}.{table}.{}", column.column_name()),
                None => format!("{table}.{}", column.column_name()),
            }
        }
        _ => column.column_name().to_string(),
    }
}

/// The table, with its schema, of model `M`: what a builder of `M` passes to
/// [`column_reference`] as its own.
pub(crate) fn model_table<M: crate::model::ModelMeta>()
-> Option<(&'static str, Option<&'static str>)> {
    Some((M::table_name(), M::schema_name()))
}

/// The builders whose `where_*` methods [`column_reference`] qualifies for.
pub(crate) trait ConditionOwner {
    /// The table of the builder's model, if it has one.
    fn own_table() -> Option<(&'static str, Option<&'static str>)>;
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

    fn column_schema(&self) -> Option<&str> {
        self.schema
    }
}

/// A strongly-typed column reference
///
/// This provides compile-time type safety for column operations.
/// The type parameter `T` represents the Rust type of the column.
#[derive(Debug, Clone, Copy)]
pub struct Column<T> {
    table: Option<&'static str>,
    schema: Option<&'static str>,
    name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T> Column<T> {
    /// Create a new typed column reference
    pub const fn new(name: &'static str) -> Self {
        Self {
            table: None,
            schema: None,
            name,
            _phantom: PhantomData,
        }
    }

    /// A column of `table`, as a model's generated `columns` name theirs: a
    /// query of another model refers to it as `table.name`.
    pub const fn of(table: &'static str, name: &'static str) -> Self {
        Self::of_schema(None, table, name)
    }

    /// A column whose owning table has an optional declared schema.
    pub const fn of_schema(
        schema: Option<&'static str>,
        table: &'static str,
        name: &'static str,
    ) -> Self {
        Self {
            table: Some(table),
            schema,
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
/// [`QueryBuilder::where_col`](crate::query::QueryBuilder::where_col): the
/// condition a query's `where_*` methods build, with the column qualified by
/// its model's table.
pub type ColumnCondition = crate::query::WhereCondition;

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
