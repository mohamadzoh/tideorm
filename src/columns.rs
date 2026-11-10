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
//! # Typed assignments and reads
//!
//! A batch update's `set()` takes the values a filter takes, and `pluck()`
//! and `value()` read a typed column as its field's type:
//!
//! ```
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool, nickname: Option<String> }
//! let _update = User::update_all().set(User::ACTIVE, false).where_eq(User::ID, 1);
//! async fn read() -> tideorm::Result<(Vec<i64>, Option<Option<String>>)> {
//!     let ids = User::query().pluck(User::ID).await?;
//!     let nickname = User::query().value(User::NICKNAME).await?;
//!     Ok((ids, nickname))
//! }
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! let _update = User::update_all().set(User::ACTIVE, "yes").where_eq(User::ID, 1);
//! ```
//!
//! ```compile_fail
//! use tideorm::prelude::*;
//! #[tideorm::model(table = "users")]
//! struct User { #[tideorm(primary_key)] id: i64, active: bool }
//! async fn read() -> tideorm::Result<Vec<String>> {
//!     User::query().pluck(User::ID).await
//! }
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

/// A list of columns, as [`select`](crate::query::QueryBuilder::select),
/// [`on_conflict`](crate::model::Model::on_conflict) and the CTE builders
/// take it.
///
/// An array, a `Vec` or a slice holds one column type: names or field names
/// (`["id", "email"]`), or typed columns of one Rust type. A tuple mixes
/// them, typed columns of different types included: `(User::ID, User::EMAIL)`.
/// A single column is a list of one, and `()` is a list of none (a window
/// function with no partition). Collect an iterator first:
/// `names.iter().collect::<Vec<_>>()`.
///
/// ```
/// use tideorm::prelude::*;
/// # #[tideorm::model(table = "users")]
/// # struct User { #[tideorm(primary_key)] id: i64, email: String }
/// User::query().select((User::ID, User::EMAIL)).validate().unwrap();
/// User::query().select(["id", "email"]).validate().unwrap();
/// User::query().select(vec![String::from("id")]).validate().unwrap();
/// User::query().select(User::EMAIL).validate().unwrap();
/// ```
pub trait IntoColumnNames {
    /// Call `visit` with each column, in order.
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName));
}

impl<C: IntoColumnName> IntoColumnNames for [C] {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        for column in self {
            visit(column);
        }
    }
}

impl<C: IntoColumnName, const N: usize> IntoColumnNames for [C; N] {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        self.as_slice().visit_columns(visit);
    }
}

impl<C: IntoColumnName> IntoColumnNames for Vec<C> {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        self.as_slice().visit_columns(visit);
    }
}

impl<T: IntoColumnNames + ?Sized> IntoColumnNames for &T {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        (**self).visit_columns(visit);
    }
}

impl IntoColumnNames for &str {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        visit(self);
    }
}

impl IntoColumnNames for String {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        visit(self);
    }
}

impl<T> IntoColumnNames for Column<T> {
    fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
        visit(self);
    }
}

impl IntoColumnNames for () {
    fn visit_columns(&self, _visit: &mut dyn FnMut(&dyn IntoColumnName)) {}
}

macro_rules! tuple_column_names {
    ($($column:ident)+) => {
        impl<$($column: IntoColumnName),+> IntoColumnNames for ($($column,)+) {
            #[allow(non_snake_case)]
            fn visit_columns(&self, visit: &mut dyn FnMut(&dyn IntoColumnName)) {
                let ($($column,)+) = self;
                $(visit($column);)+
            }
        }
    };
}
tuple_column_names!(A);
tuple_column_names!(A B);
tuple_column_names!(A B C);
tuple_column_names!(A B C D);
tuple_column_names!(A B C D E);
tuple_column_names!(A B C D E F);
tuple_column_names!(A B C D E F G);
tuple_column_names!(A B C D E F G H);
tuple_column_names!(A B C D E F G H I);
tuple_column_names!(A B C D E F G H I J);
tuple_column_names!(A B C D E F G H I J K);
tuple_column_names!(A B C D E F G H I J K L);

/// Each of `columns` through `f`, in order.
pub(crate) fn map_columns<R>(
    columns: &(impl IntoColumnNames + ?Sized),
    mut f: impl FnMut(&dyn IntoColumnName) -> R,
) -> Vec<R> {
    let mut mapped = Vec::new();
    columns.visit_columns(&mut |column| mapped.push(f(column)));
    mapped
}

/// The names of `columns`, without their tables.
pub(crate) fn column_names(columns: &(impl IntoColumnNames + ?Sized)) -> Vec<String> {
    map_columns(columns, |column| column.column_name().to_string())
}

/// A column that accepts `V` as a filter value, or as the value a batch
/// update's `set()` and `coalesce()` write.
///
/// A generated `Column<T>` accepts a `T` or a `&T`, so a borrowed list works
/// in `where_in`. A nullable `Column<Option<T>>` accepts those as well as an
/// `Option<T>`. Text columns accept `&str`, and integer, float and decimal
/// columns accept narrower numbers that convert losslessly. String column
/// names take any value a column can hold, each bound as its own type, or a
/// `serde_json::Value`, which is read as the column's type.
#[diagnostic::on_unimplemented(
    message = "a `{Self}` does not accept a `{V}` value",
    label = "expected the column's Rust type",
    note = "typed columns take their field's type or a reference to it, `&str` for text, or a losslessly widened number; name the column by string to pass any other value a column can hold"
)]
pub trait ColumnValue<V>: IntoColumnName {
    /// The value bound as the query parameter.
    type Value: Into<crate::DbValue>;
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
    [T: Into<crate::DbValue>] T => T,
    [T: Into<crate::DbValue>] T => Option<T>,
    ['a] &'a str => String,
    ['a] &'a str => Option<String>,
);

// A borrowed value binds as a copy of itself.
macro_rules! borrowed_column_value {
    ($($column:ty),* $(,)?) => {$(
        impl<'a, T: Clone + Into<crate::DbValue>> ColumnValue<&'a T> for Column<$column> {
            type Value = T;
            fn convert_value(value: &'a T) -> T { value.clone() }
        }
    )*};
}
borrowed_column_value!(T, Option<T>);

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
        impl<V: Into<crate::DbValue>> ColumnValue<V> for $ty {
            type Value = V;
            fn convert_value(value: V) -> V { value }
        }
    )*};
}
dynamic_column_value!(&str, String, &String);

/// A column whose values [`pluck()`](crate::query::QueryBuilder::pluck) and
/// [`value()`](crate::query::QueryBuilder::value) read as `T`.
///
/// A generated `Column<T>` reads as its field's type, so `pluck(User::ID)`
/// needs no annotation and a nullable column reads as an `Option`. A column
/// named by a string reads as whatever type you ask for.
#[diagnostic::on_unimplemented(
    message = "a `{Self}` does not read as `{T}`",
    label = "expected the column's Rust type",
    note = "a typed column reads as its field's type; name the column by string to read it as another type"
)]
pub trait ColumnRead<T>: IntoColumnName {}

impl<T> ColumnRead<T> for Column<T> {}

macro_rules! dynamic_column_read {
    ($($ty:ty),*) => {$(impl<T> ColumnRead<T> for $ty {})*};
}
dynamic_column_read!(&str, String, &String);

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

/// `column`, a reference [`column_reference`] qualified, without its table
/// when that is `own_table`: a typed condition (`User::ID.gt(1)`) on a query
/// of its own model names the column alone.
pub(crate) fn unqualified_own_column(
    column: String,
    own_table: Option<(&str, Option<&str>)>,
) -> String {
    let Some((own, own_schema)) = own_table else {
        return column;
    };
    let Some((table, name)) = column.rsplit_once('.') else {
        return column;
    };
    let owned = match table.split_once('.') {
        Some((schema, table)) => Some(schema) == own_schema && table == own,
        None => table == own,
    };
    if owned { name.to_string() } else { column }
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

    /// The condition on `column` as this builder writes it.
    fn condition(
        column: &impl IntoColumnName,
        operator: crate::query::Operator,
        value: crate::query::ConditionValue,
    ) -> crate::query::WhereCondition {
        crate::query::WhereCondition {
            column: column_reference(column, Self::own_table()),
            operator,
            value,
        }
    }
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

// A reference names what it points at: `&String`, `&&str` (an element of a
// `&[&str]`) and `&Column<T>` all work where a column is expected.
impl<T: IntoColumnName + ?Sized> IntoColumnName for &T {
    fn column_name(&self) -> &str {
        (**self).column_name()
    }

    fn column_table(&self) -> Option<&str> {
        (**self).column_table()
    }

    fn column_schema(&self) -> Option<&str> {
        (**self).column_schema()
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
