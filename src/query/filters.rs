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
                => Eq, $crate::query::ConditionValue::Single($crate::query::filter_value(value));
            [where_not, or_where_not, and_where_not]
                "Match rows where `column` differs from `value`; like SQL `<>`, a NULL column never matches."
                (value: impl serde::Serialize)
                => NotEq, $crate::query::ConditionValue::Single($crate::query::filter_value(value));
            [where_gt, or_where_gt, and_where_gt]
                "Match rows where `column` is greater than `value`."
                (value: impl serde::Serialize)
                => Gt, $crate::query::ConditionValue::Single($crate::query::filter_value(value));
            [where_gte, or_where_gte, and_where_gte]
                "Match rows where `column` is greater than or equal to `value`."
                (value: impl serde::Serialize)
                => Gte, $crate::query::ConditionValue::Single($crate::query::filter_value(value));
            [where_lt, or_where_lt, and_where_lt]
                "Match rows where `column` is less than `value`."
                (value: impl serde::Serialize)
                => Lt, $crate::query::ConditionValue::Single($crate::query::filter_value(value));
            [where_lte, or_where_lte, and_where_lte]
                "Match rows where `column` is less than or equal to `value`."
                (value: impl serde::Serialize)
                => Lte, $crate::query::ConditionValue::Single($crate::query::filter_value(value));
            [where_like, or_where_like, and_where_like]
                "Match rows against a raw `LIKE` pattern, used as written: `%` and `_` stay wildcards."
                (pattern: &str)
                => Like, $crate::query::ConditionValue::Single(serde_json::Value::String(pattern.to_string()));
            [where_not_like, or_where_not_like, and_where_not_like]
                "Exclude rows matching a raw `LIKE` pattern, used as written."
                (pattern: &str)
                => NotLike, $crate::query::ConditionValue::Single(serde_json::Value::String(pattern.to_string()));
            [where_contains, or_where_contains, and_where_contains]
                "Match rows where `column` contains `value` literally; its `%` and `_` are escaped, so it is safe for user input."
                (value: &str)
                => LikeEscaped, $crate::query::ConditionValue::Single(serde_json::Value::String(
                    format!("%{}%", $crate::columns::escape_like_literal(value)),
                ));
            [where_starts_with, or_where_starts_with, and_where_starts_with]
                "Match rows where `column` starts with `value` literally; its `%` and `_` are escaped."
                (value: &str)
                => LikeEscaped, $crate::query::ConditionValue::Single(serde_json::Value::String(
                    format!("{}%", $crate::columns::escape_like_literal(value)),
                ));
            [where_ends_with, or_where_ends_with, and_where_ends_with]
                "Match rows where `column` ends with `value` literally; its `%` and `_` are escaped."
                (value: &str)
                => LikeEscaped, $crate::query::ConditionValue::Single(serde_json::Value::String(
                    format!("%{}", $crate::columns::escape_like_literal(value)),
                ));
            [where_in, or_where_in, and_where_in] <V: serde::Serialize>
                "Match rows where `column` is one of `values`, each bound as its own parameter."
                (values: Vec<V>)
                => In, $crate::query::ConditionValue::List(values.into_iter().map($crate::query::filter_value).collect());
            [where_not_in, or_where_not_in, and_where_not_in] <V: serde::Serialize>
                "Match rows where `column` is none of `values`; like SQL `NOT IN`, a NULL column never matches."
                (values: Vec<V>)
                => NotIn, $crate::query::ConditionValue::List(values.into_iter().map($crate::query::filter_value).collect());
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
                => Between, $crate::query::ConditionValue::Range(
                    $crate::query::filter_value(min),
                    $crate::query::filter_value(max),
                );
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
                column: column.column_name().to_string(),
                operator: $crate::query::Operator::$operator,
                value: $value,
            })
        }
    };

    (@raw raw where => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@raw_method where_raw, $push, $doc);
    };
    (@raw raw or_where => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@raw_method or_where_raw, $push, $doc);
    };
    (@raw raw and_where => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@raw_method and_where_raw, $push, $doc);
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

mod or_branch_builder;
mod or_group;

pub use or_branch_builder::OrBranchBuilder;
pub use or_group::OrGroup;

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
    EqAny,
    NeAll,
}

/// A single where condition
#[derive(Debug, Clone)]
pub struct WhereCondition {
    pub column: String,
    pub operator: Operator,
    pub value: ConditionValue,
}

impl WhereCondition {
    pub(crate) fn new(
        column: impl crate::columns::IntoColumnName,
        operator: Operator,
        value: ConditionValue,
    ) -> Self {
        Self {
            column: column.column_name().to_string(),
            operator,
            value,
        }
    }
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
#[derive(Debug, Clone)]
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
/// a `NOT NULL` column.
///
/// Their positive duals (`IN ()`, `= ANY ()`, `&& ()`) render constant-*false*
/// and are deliberately absent: a mutation that matches nothing is safe.
///
/// The check is structural on purpose. Inspecting the rendered SQL cannot
/// replace it: sea-query emits an empty `NOT IN` as the bound pair `? = ?`
/// rather than the literal `1 = 1`, and a soft-delete model appends its own
/// `deleted_at IS NULL` conjunct to whatever the caller declared — so neither
/// shape survives a comparison against a fixed set of constant-true spellings.
pub(crate) fn condition_is_vacuous(condition: &WhereCondition) -> bool {
    match (&condition.operator, &condition.value) {
        (Operator::NotIn | Operator::NeAll, ConditionValue::List(values)) => {
            values.iter().all(serde_json::Value::is_null)
        }
        (Operator::ArrayContains, ConditionValue::List(values)) => values.is_empty(),
        _ => false,
    }
}
