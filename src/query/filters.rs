#![allow(missing_docs)]

use super::QueryBuilder;
use crate::internal::Value;
use crate::model::Model;

/// Emit the condition-builder methods of one or more families on a type.
///
/// Every builder that collects `WhereCondition`s — `QueryBuilder`, `OrGroup`,
/// `OrBranchBuilder` and `BatchUpdateBuilder` — gets its `where_*`,
/// `or_where_*` or `and_where_*` methods from the single operation table in
/// `@family` below, so the families cannot drift apart. Each family names the
/// target method that receives the built condition and the sentence that ends
/// every method's docs; `+ raw` adds the family's `*_raw` method.
///
/// ```ignore
/// condition_methods! {
///     impl[M: Model] QueryBuilder<M> {
///         or_where + raw => push_or_condition,
///             "Joins this query's single OR group, which is ANDed with its other filters.";
///     }
/// }
/// ```
macro_rules! condition_methods {
    (impl[$($generics:tt)*] $target:ty {
        $($family:ident $(+ $raw:ident)? => $push:ident, $doc:literal;)+
    }) => {
        impl<$($generics)*> $target {
            $($crate::query::condition_methods!(@family $family $(+ $raw)? => $push, $doc);)+
        }
    };

    (@family $family:ident $(+ $raw:ident)? => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@ops $family, $push, $doc;
            [where_eq, or_where_eq, and_where_eq]
                "Match rows where `column` equals `value`."
                (value: impl serde::Serialize)
                => Eq, $crate::query::ConditionValue::single(value);
            [where_not, or_where_not, and_where_not]
                "Match rows where `column` differs from `value`; like SQL `<>`, a NULL column never matches."
                (value: impl serde::Serialize)
                => NotEq, $crate::query::ConditionValue::single(value);
            [where_gt, or_where_gt, and_where_gt]
                "Match rows where `column` is greater than `value`."
                (value: impl serde::Serialize)
                => Gt, $crate::query::ConditionValue::single(value);
            [where_gte, or_where_gte, and_where_gte]
                "Match rows where `column` is greater than or equal to `value`."
                (value: impl serde::Serialize)
                => Gte, $crate::query::ConditionValue::single(value);
            [where_lt, or_where_lt, and_where_lt]
                "Match rows where `column` is less than `value`."
                (value: impl serde::Serialize)
                => Lt, $crate::query::ConditionValue::single(value);
            [where_lte, or_where_lte, and_where_lte]
                "Match rows where `column` is less than or equal to `value`."
                (value: impl serde::Serialize)
                => Lte, $crate::query::ConditionValue::single(value);
            [where_like, or_where_like, and_where_like]
                "Match rows against a raw `LIKE` pattern, used as written: `%` and `_` stay wildcards."
                (pattern: &str)
                => Like, $crate::query::ConditionValue::text(pattern);
            [where_not_like, or_where_not_like, and_where_not_like]
                "Exclude rows matching a raw `LIKE` pattern, used as written."
                (pattern: &str)
                => NotLike, $crate::query::ConditionValue::text(pattern);
            [where_contains, or_where_contains, and_where_contains]
                "Match rows where `column` contains `value` literally; its `%` and `_` are escaped, so it is safe for user input."
                (value: &str)
                => LikeEscaped, $crate::query::ConditionValue::contains(value);
            [where_starts_with, or_where_starts_with, and_where_starts_with]
                "Match rows where `column` starts with `value` literally; its `%` and `_` are escaped."
                (value: &str)
                => LikeEscaped, $crate::query::ConditionValue::starts_with(value);
            [where_ends_with, or_where_ends_with, and_where_ends_with]
                "Match rows where `column` ends with `value` literally; its `%` and `_` are escaped."
                (value: &str)
                => LikeEscaped, $crate::query::ConditionValue::ends_with(value);
            [where_in, or_where_in, and_where_in] <V: serde::Serialize>
                "Match rows where `column` is one of `values` — a `Vec`, an array, `&ids`, a set — each bound as its own parameter."
                (values: impl IntoIterator<Item = V>)
                => In, $crate::query::ConditionValue::list(values);
            [where_not_in, or_where_not_in, and_where_not_in] <V: serde::Serialize>
                "Match rows where `column` is none of `values`, any list as for `where_in`; like SQL `NOT IN`, a NULL column never matches."
                (values: impl IntoIterator<Item = V>)
                => NotIn, $crate::query::ConditionValue::list(values);
            [where_null, or_where_null, and_where_null]
                "Match rows where `column` is NULL."
                ()
                => IsNull, $crate::query::ConditionValue::None;
            [where_not_null, or_where_not_null, and_where_not_null]
                "Match rows where `column` holds a value."
                ()
                => IsNotNull, $crate::query::ConditionValue::None;
            [where_between, or_where_between, and_where_between]
                "Match rows where `column` lies between `min` and `max`, inclusive."
                (min: impl serde::Serialize, max: impl serde::Serialize)
                => Between, $crate::query::ConditionValue::range(min, max);
            [where_not_between, or_where_not_between, and_where_not_between]
                "Match rows where `column` lies outside `min` and `max`; like SQL `NOT BETWEEN`, a NULL column never matches."
                (min: impl serde::Serialize, max: impl serde::Serialize)
                => NotBetween, $crate::query::ConditionValue::range(min, max);
            [where_column_eq, or_where_column_eq, and_where_column_eq]
                "Match rows where `column` equals the row's `other` column: `where_column_eq(\"shipped_on\", \"ordered_on\")`."
                (other: impl $crate::columns::IntoColumnName)
                => Eq, $crate::query::ConditionValue::Column($crate::columns::column_reference(
                    &other,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ));
            [where_column_ne, or_where_column_ne, and_where_column_ne]
                "Match rows where `column` differs from the row's `other` column; a NULL in either never matches."
                (other: impl $crate::columns::IntoColumnName)
                => NotEq, $crate::query::ConditionValue::Column($crate::columns::column_reference(
                    &other,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ));
            [where_column_gt, or_where_column_gt, and_where_column_gt]
                "Match rows where `column` is greater than the row's `other` column: `where_column_gt(\"updated_at\", \"created_at\")`."
                (other: impl $crate::columns::IntoColumnName)
                => Gt, $crate::query::ConditionValue::Column($crate::columns::column_reference(
                    &other,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ));
            [where_column_gte, or_where_column_gte, and_where_column_gte]
                "Match rows where `column` is at least the row's `other` column."
                (other: impl $crate::columns::IntoColumnName)
                => Gte, $crate::query::ConditionValue::Column($crate::columns::column_reference(
                    &other,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ));
            [where_column_lt, or_where_column_lt, and_where_column_lt]
                "Match rows where `column` is less than the row's `other` column."
                (other: impl $crate::columns::IntoColumnName)
                => Lt, $crate::query::ConditionValue::Column($crate::columns::column_reference(
                    &other,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ));
            [where_column_lte, or_where_column_lte, and_where_column_lte]
                "Match rows where `column` is at most the row's `other` column."
                (other: impl $crate::columns::IntoColumnName)
                => Lte, $crate::query::ConditionValue::Column($crate::columns::column_reference(
                    &other,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ));
            [where_json_contains, or_where_json_contains, and_where_json_contains]
                "Match rows whose JSON `column` contains `value`: every key and element of `value` is in it, as PostgreSQL's `@>` reads it, on every backend."
                (value: impl serde::Serialize)
                => JsonContains, $crate::query::ConditionValue::single(value);
            [where_json_contained_by, or_where_json_contained_by, and_where_json_contained_by]
                "Match rows whose JSON `column` is contained by `value`, as PostgreSQL's `<@` reads it."
                (value: impl serde::Serialize)
                => JsonContainedBy, $crate::query::ConditionValue::single(value);
            [where_json_key_exists, or_where_json_key_exists, and_where_json_key_exists]
                "Match rows whose JSON `column` has the top-level `key`, one holding JSON `null` included; a NULL column matches neither this nor `where_json_key_not_exists`."
                (key: &str)
                => JsonKeyExists, $crate::query::ConditionValue::text(key);
            [where_json_key_not_exists, or_where_json_key_not_exists, and_where_json_key_not_exists]
                "Match rows whose JSON `column` lacks the top-level `key`; a NULL column matches neither this nor `where_json_key_exists`."
                (key: &str)
                => JsonKeyNotExists, $crate::query::ConditionValue::text(key);
            [where_json_path_exists, or_where_json_path_exists, and_where_json_path_exists]
                "Match rows whose JSON `column` has a member at `path` (`a.b.c`), one holding JSON `null` included."
                (path: &str)
                => JsonPathExists, $crate::query::ConditionValue::text(path);
            [where_json_path_not_exists, or_where_json_path_not_exists, and_where_json_path_not_exists]
                "Match rows whose JSON `column` has no member at `path`; a NULL column matches neither this nor `where_json_path_exists`."
                (path: &str)
                => JsonPathNotExists, $crate::query::ConditionValue::text(path);
            [where_array_contains, or_where_array_contains, and_where_array_contains] <V: serde::Serialize>
                "Match rows whose array `column` holds every one of `values` (`@>`), any list of any serializable value."
                (values: impl IntoIterator<Item = V>)
                => ArrayContains, $crate::query::ConditionValue::list(values);
            [where_array_contained_by, or_where_array_contained_by, and_where_array_contained_by] <V: serde::Serialize>
                "Match rows whose array `column` holds nothing but `values` (`<@`); a NULL in the list allows NULL elements."
                (values: impl IntoIterator<Item = V>)
                => ArrayContainedBy, $crate::query::ConditionValue::list(values);
            [where_array_overlaps, or_where_array_overlaps, and_where_array_overlaps] <V: serde::Serialize>
                "Match rows whose array `column` holds at least one of `values` (`&&`)."
                (values: impl IntoIterator<Item = V>)
                => ArrayOverlaps, $crate::query::ConditionValue::list(values);
        );
        $($crate::query::condition_methods!(@raw $raw $family => $push, $doc);)?
    };

    (@ops where, $push:ident, $doc:literal; $(
        [$name:ident, $or_name:ident, $and_name:ident] $(<$generic:ident: $bound:path>)?
        $summary:literal ($($arg:ident: $arg_ty:ty),*) => $operator:ident, $value:expr;
    )*) => {$(
        $crate::query::condition_methods!(@method $name $(<$generic: $bound>)?, $summary, $doc,
            ($($arg: $arg_ty),*), $push, $operator, $value);
    )*};

    (@ops or_where, $push:ident, $doc:literal; $(
        [$name:ident, $or_name:ident, $and_name:ident] $(<$generic:ident: $bound:path>)?
        $summary:literal ($($arg:ident: $arg_ty:ty),*) => $operator:ident, $value:expr;
    )*) => {$(
        $crate::query::condition_methods!(@method $or_name $(<$generic: $bound>)?, $summary, $doc,
            ($($arg: $arg_ty),*), $push, $operator, $value);
    )*};

    (@ops and_where, $push:ident, $doc:literal; $(
        [$name:ident, $or_name:ident, $and_name:ident] $(<$generic:ident: $bound:path>)?
        $summary:literal ($($arg:ident: $arg_ty:ty),*) => $operator:ident, $value:expr;
    )*) => {$(
        $crate::query::condition_methods!(@method $and_name $(<$generic: $bound>)?, $summary, $doc,
            ($($arg: $arg_ty),*), $push, $operator, $value);
    )*};

    (@method $name:ident $(<$generic:ident: $bound:path>)?, $summary:literal, $doc:literal,
        ($($arg:ident: $arg_ty:ty),*), $push:ident, $operator:ident, $value:expr) => {
        #[doc = $summary]
        #[doc = ""]
        #[doc = $doc]
        #[must_use]
        pub fn $name $(<$generic: $bound>)? (
            self,
            column: impl $crate::columns::IntoColumnName,
            $($arg: $arg_ty),*
        ) -> Self {
            self.$push($crate::query::WhereCondition {
                column: $crate::columns::column_reference(
                    &column,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ),
                operator: $crate::query::Operator::$operator,
                value: $value,
            })
        }
    };

    (@raw raw where => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@raw_method where_raw, $push, $doc);
        $crate::query::condition_methods!(@raw_with_method where_raw_with, $push, $doc);
    };
    (@raw raw or_where => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@raw_method or_where_raw, $push, $doc);
        $crate::query::condition_methods!(@raw_with_method or_where_raw_with, $push, $doc);
    };
    (@raw raw and_where => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@raw_method and_where_raw, $push, $doc);
        $crate::query::condition_methods!(@raw_with_method and_where_raw_with, $push, $doc);
    };

    (@raw_with_method $name:ident, $push:ident, $doc:literal) => {
        /// Match rows by a raw SQL fragment whose `?` placeholders bind
        /// `params`, in order.
        ///
        /// The fragment is **trusted SQL**, checked like the `*_raw` form's;
        /// the values are bound, never written into it, so they may come from
        /// a request. Write `?` on every backend. A `?` inside a quoted literal
        /// is text, and a count of placeholders other than `params.len()`
        /// fails the query.
        ///
        #[doc = $doc]
        #[must_use]
        pub fn $name(self, raw_sql: &str, params: Vec<$crate::internal::DbValue>) -> Self {
            self.$push($crate::query::WhereCondition {
                column: String::new(),
                operator: $crate::query::Operator::Raw,
                value: $crate::query::ConditionValue::RawTemplate {
                    sql: raw_sql.to_string(),
                    values: params.into_iter().map($crate::internal::bindable_value).collect(),
                },
            })
        }
    };

    (@raw_method $name:ident, $push:ident, $doc:literal) => {
        /// Match rows by a raw SQL fragment.
        ///
        /// **Trusted SQL only**: the fragment is checked for statement
        /// separators, comments and unbalanced quotes before the query runs, and
        /// a rejected one fails the query, but it is never escaped.
        ///
        #[doc = $doc]
        #[must_use]
        pub fn $name(self, raw_sql: &str) -> Self {
            self.$push($crate::query::WhereCondition {
                column: String::new(),
                operator: $crate::query::Operator::Raw,
                value: $crate::query::ConditionValue::RawExpr(raw_sql.to_string()),
            })
        }
    };
}

pub(crate) use condition_methods;

/// Emit `when` and `when_some` on a builder.
macro_rules! conditional_methods {
    (impl[$($generics:tt)*] $target:ty) => {
        impl<$($generics)*> $target {
            /// Apply `f` only when `condition` holds: a filter that depends on
            /// a request parameter, without breaking the chain.
            ///
            /// ```ignore
            /// query.when(only_active, |q| q.where_eq("active", true))
            /// ```
            #[must_use]
            pub fn when(self, condition: bool, f: impl FnOnce(Self) -> Self) -> Self {
                if condition { f(self) } else { self }
            }

            /// Apply `f` with the value when `option` holds one.
            ///
            /// ```ignore
            /// query.when_some(params.role, |q, role| q.where_eq("role", role))
            /// ```
            #[must_use]
            pub fn when_some<T>(self, option: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
                match option {
                    Some(value) => f(self, value),
                    None => self,
                }
            }
        }
    };
}

conditional_methods!(impl[M: Model] QueryBuilder<M>);
conditional_methods!(impl[] OrGroup);
conditional_methods!(impl[M: Model] OrBranchBuilder<M>);
conditional_methods!(impl[M: Model] crate::model::BatchUpdateBuilder<M>);
conditional_methods!(impl[M: Model] crate::relations::EagerQueryBuilder<M>);

mod finite;
mod or_branch_builder;
mod or_group;

pub(crate) use finite::checked_filter_value;

pub use or_branch_builder::OrBranchBuilder;
pub use or_group::OrGroup;

impl OrGroup {
    /// Add every condition of the group, its nested groups' included, to
    /// `conditions`.
    fn collect_conditions<'a>(&'a self, conditions: &mut Vec<&'a WhereCondition>) {
        conditions.extend(&self.conditions);
        for nested in &self.nested_groups {
            nested.collect_conditions(conditions);
        }
    }
}

impl<M: Model> QueryBuilder<M> {
    /// Every condition of the query: its own, then each OR group's, nested
    /// ones included.
    pub(crate) fn all_conditions(&self) -> Vec<&WhereCondition> {
        let mut conditions: Vec<&WhereCondition> = self.clauses.conditions.iter().collect();
        for group in &self.clauses.or_groups {
            group.collect_conditions(&mut conditions);
        }
        conditions
    }
}

/// Sort order for queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Order {
    /// Ascending order (A-Z, 1-9)
    Asc,
    /// Descending order (Z-A, 9-1)
    Desc,
}

/// [`Order`] under a second name, for a crate whose own `Order` type (an
/// `Order` model, say) shadows the prelude's: `SortOrder::Desc`.
pub type SortOrder = Order;

impl Order {
    /// The SQL keyword for this direction: `ASC` or `DESC`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

/// Comparison operators for where clauses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operator {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    LikeEscaped,
    NotLike,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
    NotBetween,
    JsonContains,
    JsonContainedBy,
    JsonKeyExists,
    JsonKeyNotExists,
    JsonPathExists,
    JsonPathNotExists,
    ArrayContains,
    ArrayContainedBy,
    ArrayOverlaps,
    Raw,
}

/// A single where condition
#[derive(Debug, Clone)]
pub struct WhereCondition {
    pub column: String,
    pub operator: Operator,
    pub value: ConditionValue,
}

/// A filter or assignment value as the JSON the builders carry.
///
/// Anything `Serialize` is accepted, so `Uuid`, `chrono` and `Decimal` values
/// pass as they are; the SQL renderers bind them back as their column's type.
///
/// # Panics
///
/// When `value`'s `Serialize` implementation fails, which no type a column can
/// hold does — the same contract as `serde_json::json!`.
pub(crate) fn filter_value(value: impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(value).expect("a filter value must serialize to JSON")
}

/// Value for a where condition
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionValue {
    Single(serde_json::Value),
    List(Vec<serde_json::Value>),
    Range(serde_json::Value, serde_json::Value),
    None,
    RawExpr(String),
    /// A builder-generated SQL fragment that carries its own bound parameters.
    ///
    /// Unlike [`ConditionValue::RawExpr`], which is emitted verbatim through
    /// `Expr::cust`, this variant keeps the operand's values out of the SQL text
    /// entirely. It exists for the operands the builder generates itself —
    /// `where_in_subquery`, `where_exists`, the `has_related` family — where the
    /// only alternative is hand-escaping user data into an inline literal.
    RawExprWithValues {
        /// The executable fragment.
        ///
        /// It must already use the *target backend's own* placeholder marker
        /// (`$1..$n` on PostgreSQL, `?` on MySQL/MariaDB/SQLite), because
        /// `Expr::cust_with_values` only renumbers tokens matching that marker
        /// into the surrounding statement; a mismatched marker binds nothing.
        sql: String,
        /// The parameters bound to `sql`, in placeholder order.
        values: Vec<Value>,
    },
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
            Self::Single(value) => write!(f, "{}", value),
            Self::List(values) => write!(f, "{}", serde_json::Value::Array(values.clone())),
            Self::Range(low, high) => write!(f, "{}..{}", low, high),
            Self::None => f.write_str("NULL"),
            Self::RawExpr(sql)
            | Self::RawExprWithValues { sql, .. }
            | Self::RawTemplate { sql, .. } => f.write_str(sql),
            Self::Column(column) => f.write_str(column),
            Self::Invalid(reason) => write!(f, "<invalid: {}>", reason),
        }
    }
}

impl ConditionValue {
    /// One filter value; [`Invalid`](Self::Invalid) for one SQL cannot compare.
    pub(crate) fn single(value: impl serde::Serialize) -> Self {
        checked_filter_value(value).map_or_else(Self::Invalid, Self::Single)
    }

    /// A list of filter values, refused as a whole when one cannot be compared.
    pub(crate) fn list<V: serde::Serialize>(values: impl IntoIterator<Item = V>) -> Self {
        values
            .into_iter()
            .map(checked_filter_value)
            .collect::<Result<Vec<_>, _>>()
            .map_or_else(Self::Invalid, Self::List)
    }

    /// The bounds of a range.
    pub(crate) fn range(low: impl serde::Serialize, high: impl serde::Serialize) -> Self {
        match (checked_filter_value(low), checked_filter_value(high)) {
            (Ok(low), Ok(high)) => Self::Range(low, high),
            (Err(reason), _) | (_, Err(reason)) => Self::Invalid(reason),
        }
    }

    /// One text value: a `LIKE` pattern, a JSON key or path.
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self::Single(serde_json::Value::String(text.into()))
    }

    /// The `LIKE` pattern matching text that contains `value` literally.
    pub(crate) fn contains(value: &str) -> Self {
        Self::text(format!("%{}%", crate::columns::escape_like_literal(value)))
    }

    /// The `LIKE` pattern matching text that starts with `value` literally.
    pub(crate) fn starts_with(value: &str) -> Self {
        Self::text(format!("{}%", crate::columns::escape_like_literal(value)))
    }

    /// The `LIKE` pattern matching text that ends with `value` literally.
    pub(crate) fn ends_with(value: &str) -> Self {
        Self::text(format!("%{}", crate::columns::escape_like_literal(value)))
    }
}

/// Logical operator for combining conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalOp {
    And,
    Or,
}

impl LogicalOp {
    pub fn as_sql(&self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
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
/// The check is structural on purpose. Inspecting the rendered SQL cannot
/// replace it: sea-query emits an empty `NOT IN` as the bound pair `? = ?`
/// rather than the literal `1 = 1`, and a soft-delete model appends its own
/// `deleted_at IS NULL` conjunct to whatever the caller declared — so neither
/// shape survives a comparison against a fixed set of constant-true spellings.
pub(crate) fn condition_is_vacuous(condition: &WhereCondition) -> bool {
    match (&condition.operator, &condition.value) {
        (Operator::NotIn, ConditionValue::List(values)) => {
            values.iter().all(serde_json::Value::is_null)
        }
        // `where_not(col, None)` renders the same `col IS NOT NULL`.
        (Operator::NotEq, ConditionValue::Single(serde_json::Value::Null)) => true,
        (Operator::ArrayContains, ConditionValue::List(values)) => values.is_empty(),
        // Every object document contains `{}`, and every array `[]`.
        (Operator::JsonContains, ConditionValue::Single(document)) => {
            document.as_object().is_some_and(serde_json::Map::is_empty)
                || document.as_array().is_some_and(Vec::is_empty)
        }
        _ => false,
    }
}
