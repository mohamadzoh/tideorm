use super::QueryBuilder;
use crate::model::Model;

// Executable examples emitted beside every generated predicate.
macro_rules! condition_example {
    ($receiver:ident, $name:ident, $operator:ident, other) => {
        $crate::query::condition_example!(@example $receiver, $name, "User::ID, User::ID")
    };
    ($receiver:ident, $name:ident, $operator:ident $(, $arg:ident)* $(,)?) => {
        $crate::query::condition_example!(@example $receiver, $name,
            $crate::query::condition_example!(@args $operator))
    };
    (@example $receiver:ident, $name:ident, $args:expr) => {
        concat!(
            "\n\n```\nuse tideorm::prelude::*;\n",
            "# #[tideorm::model(table = \"users\")]\n",
            "# struct User { #[tideorm(primary_key)] id: i64, name: String, data: serde_json::Value, tags: Vec<String> }\n",
            $crate::query::condition_example!(@start $receiver), stringify!($name), "(", $args, ")",
            $crate::query::condition_example!(@end $receiver), "\n```"
        )
    };
    (@start QueryBuilder) => { "User::query()." };
    (@end QueryBuilder) => { ".validate().unwrap();" };
    (@start OrBranchBuilder) => { "User::query().begin_or()." };
    (@end OrBranchBuilder) => { ".end_or().validate().unwrap();" };
    (@start OrGroup) => { "let group = OrGroup::new()." };
    (@end OrGroup) => { ";\nassert_eq!(group.condition_count(), 1);" };
    (@start BatchUpdateBuilder) => { "let _update = User::update_all()." };
    (@end BatchUpdateBuilder) => { ";" };
    (@args Eq) => { "User::ID, 1" };
    (@args NotEq) => { "User::ID, 1" };
    (@args Gt) => { "User::ID, 1" };
    (@args Gte) => { "User::ID, 1" };
    (@args Lt) => { "User::ID, 1" };
    (@args Lte) => { "User::ID, 1" };
    (@args Like) => { "User::NAME, \"A%\"" };
    (@args NotLike) => { "User::NAME, \"A%\"" };
    (@args Contains) => { "User::NAME, \"A%\"" };
    (@args StartsWith) => { "User::NAME, \"A%\"" };
    (@args EndsWith) => { "User::NAME, \"A%\"" };
    (@args In) => { "User::ID, [1, 2]" };
    (@args NotIn) => { "User::ID, [1, 2]" };
    (@args IsNull) => { "User::NAME" };
    (@args IsNotNull) => { "User::NAME" };
    (@args Between) => { "User::ID, 1, 10" };
    (@args NotBetween) => { "User::ID, 1, 10" };
    (@args JsonContains) => { "\"data\", serde_json::json!({\"key\": 1})" };
    (@args JsonContainedBy) => { "\"data\", serde_json::json!({\"key\": 1})" };
    (@args JsonKeyExists) => { "\"data\", \"key\"" };
    (@args JsonKeyNotExists) => { "\"data\", \"key\"" };
    (@args JsonPathExists) => { "\"data\", \"key\"" };
    (@args JsonPathNotExists) => { "\"data\", \"key\"" };
    (@args ArrayContains) => { "\"tags\", [\"rust\"]" };
    (@args ArrayContainedBy) => { "\"tags\", [\"rust\"]" };
    (@args ArrayOverlaps) => { "\"tags\", [\"rust\"]" };
    (@args Raw) => { "\"id > 0\"" };
    (@args RawWith) => { "\"id > ?\", vec![1_i64.into()]" };
}
pub(crate) use condition_example;

/// Emit the condition-builder methods of one or more families on a type.
///
/// Every builder that collects `WhereCondition`s — `QueryBuilder`, `OrGroup`,
/// `OrBranchBuilder` and `BatchUpdateBuilder` — gets its `where_*`,
/// `or_where_*` or `and_where_*` methods from the operation tables in
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
    (impl[$($generics:tt)*] $target:ident $(<$model:ident>)? {
        $($family:ident $(+ $raw:ident)? => $push:ident, $doc:literal;)+
    }) => {
        impl<$($generics)*> $target $(<$model>)? {
            $($crate::query::condition_methods!(@family $target, $family $(+ $raw)? => $push, $doc);)+
        }
    };

    (@family $receiver:ident, $family:ident $(+ $raw:ident)? => $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@typed_ops $family, $receiver, $push, $doc;
            [where_eq, or_where_eq, and_where_eq] TypedValue
                "Match rows where `column` equals `value`."
                => Eq;
            [where_not, or_where_not, and_where_not] TypedValue
                "Match rows where `column` differs from `value`; like SQL `<>`, a NULL column never matches."
                => NotEq;
            [where_ne, or_where_ne, and_where_ne] TypedValue
                "Match rows where `column` differs from `value`, as `where_not` does, under the name `where_column_ne` and `ColumnEq::ne` use."
                => NotEq;
            [where_gt, or_where_gt, and_where_gt] TypedValue
                "Match rows where `column` is greater than `value`."
                => Gt;
            [where_gte, or_where_gte, and_where_gte] TypedValue
                "Match rows where `column` is greater than or equal to `value`."
                => Gte;
            [where_lt, or_where_lt, and_where_lt] TypedValue
                "Match rows where `column` is less than `value`."
                => Lt;
            [where_lte, or_where_lte, and_where_lte] TypedValue
                "Match rows where `column` is less than or equal to `value`."
                => Lte;
            [where_in, or_where_in, and_where_in] TypedList
                "Match rows where `column` is one of `values` — a `Vec`, an array, `&ids`, a set — each bound as its own parameter."
                => In;
            [where_not_in, or_where_not_in, and_where_not_in] TypedList
                "Match rows where `column` is none of `values`, any list as for `where_in`; like SQL `NOT IN`, a NULL column never matches."
                => NotIn;
            [where_between, or_where_between, and_where_between] TypedRange
                "Match rows where `column` lies between `min` and `max`, inclusive."
                => Between;
            [where_not_between, or_where_not_between, and_where_not_between] TypedRange
                "Match rows where `column` lies outside `min` and `max`; like SQL `NOT BETWEEN`, a NULL column never matches."
                => NotBetween;
        );
        $crate::query::condition_methods!(@ops $family, $receiver, $push, $doc;
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
                => Contains, $crate::query::ConditionValue::text(value);
            [where_starts_with, or_where_starts_with, and_where_starts_with]
                "Match rows where `column` starts with `value` literally; its `%` and `_` are escaped."
                (value: &str)
                => StartsWith, $crate::query::ConditionValue::text(value);
            [where_ends_with, or_where_ends_with, and_where_ends_with]
                "Match rows where `column` ends with `value` literally; its `%` and `_` are escaped."
                (value: &str)
                => EndsWith, $crate::query::ConditionValue::text(value);
            [where_null, or_where_null, and_where_null]
                "Match rows where `column` is NULL."
                ()
                => IsNull, $crate::query::ConditionValue::None;
            [where_not_null, or_where_not_null, and_where_not_null]
                "Match rows where `column` holds a value."
                ()
                => IsNotNull, $crate::query::ConditionValue::None;
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
                => JsonContains, $crate::query::ConditionValue::document(value);
            [where_json_contained_by, or_where_json_contained_by, and_where_json_contained_by]
                "Match rows whose JSON `column` is contained by `value`, as PostgreSQL's `<@` reads it."
                (value: impl serde::Serialize)
                => JsonContainedBy, $crate::query::ConditionValue::document(value);
            [where_json_key_exists, or_where_json_key_exists, and_where_json_key_exists]
                "Match rows whose JSON `column` has the top-level `key`, one holding JSON `null` included; a NULL column matches neither this nor `where_json_key_not_exists`."
                (key: &str)
                => JsonKeyExists, $crate::query::ConditionValue::text(key);
            [where_json_key_not_exists, or_where_json_key_not_exists, and_where_json_key_not_exists]
                "Match rows whose JSON `column` lacks the top-level `key`; a NULL column matches neither this nor `where_json_key_exists`."
                (key: &str)
                => JsonKeyNotExists, $crate::query::ConditionValue::text(key);
            [where_json_path_exists, or_where_json_path_exists, and_where_json_path_exists]
                "Match rows whose JSON `column` has a member at `path` (`$.a.b[0]`, or `a.b[0]` from the root), one holding JSON `null` included. A step never looks inside an array it does not index, nor indexes a value that is not an array; a path of other steps is an error, except on PostgreSQL, which reads it as a JSONPath."
                (path: &str)
                => JsonPathExists, $crate::query::ConditionValue::json_path(path);
            [where_json_path_not_exists, or_where_json_path_not_exists, and_where_json_path_not_exists]
                "Match rows whose JSON `column` has no member at `path`, read as for `where_json_path_exists`; a NULL column matches neither."
                (path: &str)
                => JsonPathNotExists, $crate::query::ConditionValue::json_path(path);
            [where_array_contains, or_where_array_contains, and_where_array_contains] <V: Into<$crate::DbValue>>
                "Match rows whose array `column` holds every one of `values` (`@>`), any list of values."
                (values: impl IntoIterator<Item = V>)
                => ArrayContains, $crate::query::ConditionValue::list(values);
            [where_array_contained_by, or_where_array_contained_by, and_where_array_contained_by] <V: Into<$crate::DbValue>>
                "Match rows whose array `column` holds nothing but `values` (`<@`); a NULL in the list allows NULL elements."
                (values: impl IntoIterator<Item = V>)
                => ArrayContainedBy, $crate::query::ConditionValue::list(values);
            [where_array_overlaps, or_where_array_overlaps, and_where_array_overlaps] <V: Into<$crate::DbValue>>
                "Match rows whose array `column` holds at least one of `values` (`&&`)."
                (values: impl IntoIterator<Item = V>)
                => ArrayOverlaps, $crate::query::ConditionValue::list(values);
        );
        $crate::query::condition_methods!(@col $family, $receiver, $push, $doc);
        $($crate::query::condition_methods!(@raw $raw $family => $receiver, $push, $doc);)?
    };

    // Expand `@$arm` with the family's name out of a `[where, or_where,
    // and_where]` triple.
    (@pick where, [$name:ident, $or_name:ident, $and_name:ident], $arm:ident $($rest:tt)*) => {
        $crate::query::condition_methods!(@ $arm $name $($rest)*);
    };
    (@pick or_where, [$name:ident, $or_name:ident, $and_name:ident], $arm:ident $($rest:tt)*) => {
        $crate::query::condition_methods!(@ $arm $or_name $($rest)*);
    };
    (@pick and_where, [$name:ident, $or_name:ident, $and_name:ident], $arm:ident $($rest:tt)*) => {
        $crate::query::condition_methods!(@ $arm $and_name $($rest)*);
    };

    (@col $family:ident, $receiver:ident, $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@pick $family, [where_col, or_where_col, and_where_col],
            col_method, $receiver, $push, $doc);
    };

    (@col_method $name:ident, $receiver:ident, $push:ident, $doc:literal) => {
        /// Match rows by a condition built from a typed column:
        /// `User::ID.gt(1)`, `User::NAME.contains("an")`, `User::EMAIL.is_null()`.
        ///
        #[doc = $doc]
        #[doc = $crate::query::condition_example!(@example $receiver, $name, "User::ID.gt(1)")]
        pub fn $name(self, condition: $crate::columns::ColumnCondition) -> Self {
            let $crate::query::WhereCondition { column, operator, value } = condition;
            self.$push($crate::query::WhereCondition {
                column: $crate::columns::unqualified_own_column(
                    column,
                    <Self as $crate::columns::ConditionOwner>::own_table(),
                ),
                operator,
                value,
            })
        }
    };

    (@ops $family:ident, $receiver:ident, $push:ident, $doc:literal; $(
        [$name:ident, $or_name:ident, $and_name:ident] $(<$generic:ident: $bound:path>)?
        $summary:literal ($($arg:ident: $arg_ty:ty),*) => $operator:ident, $value:expr;
    )*) => {$(
        $crate::query::condition_methods!(@pick $family, [$name, $or_name, $and_name], method
            $(<$generic: $bound>)?, $summary, $doc, $receiver, ($($arg: $arg_ty),*), $push, $operator, $value);
    )*};

    (@typed_ops $family:ident, $receiver:ident, $push:ident, $doc:literal; $(
        [$name:ident, $or_name:ident, $and_name:ident] $kind:ident
        $summary:literal => $operator:ident;
    )*) => {$(
        $crate::query::condition_methods!(@pick $family, [$name, $or_name, $and_name], typed_method,
            $kind, $summary, $doc, $receiver, $push, $operator);
    )*};
    // TypedValue/TypedList/TypedRange are dispatch markers for value-aware
    // signatures. The remaining operations keep their operator-specific types.
    (@typed_method $name:ident, TypedValue, $summary:literal, $doc:literal,
        $receiver:ident, $push:ident, $operator:ident) => {
        #[doc = $summary]
        #[doc = ""]
        #[doc = $doc]
        /// Typed columns accept their Rust type (see [`ColumnValue`]); string
        /// names accept any value, bound as its own type, or a
        /// `serde_json::Value`, read as the column's.
        ///
        /// [`ColumnValue`]: crate::columns::ColumnValue
        #[doc = $crate::query::condition_example!($receiver, $name, $operator)]
        pub fn $name<C, V>(self, column: C, value: V) -> Self
        where
            C: $crate::columns::ColumnValue<V>,
        {
            self.$push(<Self as $crate::columns::ConditionOwner>::condition(
                &column,
                $crate::query::Operator::$operator,
                $crate::query::ConditionValue::single(C::convert_value(value)),
            ))
        }
    };

    (@typed_method $name:ident, TypedList, $summary:literal, $doc:literal,
        $receiver:ident, $push:ident, $operator:ident) => {
        #[doc = $summary]
        /// A typed column takes owned or borrowed elements, so `&ids` works.
        #[doc = ""]
        #[doc = $doc]
        #[doc = $crate::query::condition_example!($receiver, $name, $operator)]
        pub fn $name<C, V>(self, column: C, values: impl IntoIterator<Item = V>) -> Self
        where
            C: $crate::columns::ColumnValue<V>,
        {
            self.$push(<Self as $crate::columns::ConditionOwner>::condition(
                &column,
                $crate::query::Operator::$operator,
                $crate::query::ConditionValue::list(values.into_iter().map(C::convert_value)),
            ))
        }
    };

    (@typed_method $name:ident, TypedRange, $summary:literal, $doc:literal,
        $receiver:ident, $push:ident, $operator:ident) => {
        #[doc = $summary]
        #[doc = ""]
        #[doc = $doc]
        #[doc = $crate::query::condition_example!($receiver, $name, $operator)]
        pub fn $name<C, L, H>(self, column: C, min: L, max: H) -> Self
        where
            C: $crate::columns::ColumnValue<L> + $crate::columns::ColumnValue<H>,
        {
            self.$push(<Self as $crate::columns::ConditionOwner>::condition(
                &column,
                $crate::query::Operator::$operator,
                $crate::query::ConditionValue::range(
                    <C as $crate::columns::ColumnValue<L>>::convert_value(min),
                    <C as $crate::columns::ColumnValue<H>>::convert_value(max),
                ),
            ))
        }
    };

    (@method $name:ident $(<$generic:ident: $bound:path>)?, $summary:literal, $doc:literal, $receiver:ident,
        ($($arg:ident: $arg_ty:ty),*), $push:ident, $operator:ident, $value:expr) => {
        #[doc = $summary]
        #[doc = ""]
        #[doc = $doc]
        #[doc = $crate::query::condition_example!($receiver, $name, $operator, $($arg),*)]
        pub fn $name $(<$generic: $bound>)? (
            self,
            column: impl $crate::columns::IntoColumnName,
            $($arg: $arg_ty),*
        ) -> Self {
            self.$push(<Self as $crate::columns::ConditionOwner>::condition(
                &column,
                $crate::query::Operator::$operator,
                $value,
            ))
        }
    };

    (@raw raw $family:ident => $receiver:ident, $push:ident, $doc:literal) => {
        $crate::query::condition_methods!(@pick $family, [where_raw, or_where_raw, and_where_raw],
            raw_method, $receiver, $push, $doc);
        $crate::query::condition_methods!(@pick $family,
            [where_raw_with, or_where_raw_with, and_where_raw_with], raw_with_method, $receiver, $push, $doc);
    };

    (@raw_with_method $name:ident, $receiver:ident, $push:ident, $doc:literal) => {
        /// Match rows by a raw SQL fragment whose `?` placeholders bind
        /// `params`, in order.
        ///
        /// The fragment is **trusted SQL**, checked like the `*_raw` form's;
        /// the values are bound, never written into it, so they may come from
        /// a request. Write `?` on every backend. A `?` inside a quoted literal
        /// is text, and a count of placeholders other than the number of
        /// values fails the query. `params` is any list of values: a `Vec`,
        /// an array or an iterator.
        ///
        #[doc = $doc]
        #[doc = $crate::query::condition_example!($receiver, $name, RawWith)]
        pub fn $name(
            self,
            raw_sql: &str,
            params: impl IntoIterator<Item = $crate::internal::DbValue>,
        ) -> Self {
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

    (@raw_method $name:ident, $receiver:ident, $push:ident, $doc:literal) => {
        /// Match rows by a raw SQL fragment.
        ///
        /// **Trusted SQL only**: the fragment is checked for statement
        /// separators, comments and unbalanced quotes before the query runs, and
        /// a rejected one fails the query, but it is never escaped.
        ///
        #[doc = $doc]
        #[doc = $crate::query::condition_example!($receiver, $name, Raw)]
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
    (impl[$($generics:tt)*] $target:ty; $start:literal, $apply:literal) => {
        impl<$($generics)*> $target {
            /// Apply `f` only when `condition` holds: a filter that depends on
            /// a request parameter, without breaking the chain.
            ///
            #[doc = concat!(
                "```\nuse tideorm::prelude::*;\n",
                "# #[tideorm::model(table = \"users\")]\n",
                "# struct User { #[tideorm(primary_key)] id: i64, active: bool }\n",
                "let _query = ", $start, ".when(true, |q| { let value = true; ", $apply, " });\n```"
            )]
            pub fn when(self, condition: bool, f: impl FnOnce(Self) -> Self) -> Self {
                if condition { f(self) } else { self }
            }

            /// Apply `f` with the value when `option` holds one.
            ///
            #[doc = concat!(
                "```\nuse tideorm::prelude::*;\n",
                "# #[tideorm::model(table = \"users\")]\n",
                "# struct User { #[tideorm(primary_key)] id: i64, active: bool }\n",
                "let _query = ", $start, ".when_some(Some(true), |q, value| ", $apply, ");\n```"
            )]
            pub fn when_some<T>(self, option: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
                match option {
                    Some(value) => f(self, value),
                    None => self,
                }
            }
        }
    };
}

conditional_methods!(impl[M: Model] QueryBuilder<M>; "User::query()", "q.where_eq(User::ACTIVE, value)");
conditional_methods!(impl[] OrGroup; "OrGroup::new()", "q.where_eq(User::ACTIVE, value)");
conditional_methods!(impl[M: Model] OrBranchBuilder<M>; "User::query().begin_or()", "q.or_where_eq(User::ACTIVE, value)");
conditional_methods!(impl[M: Model] crate::model::BatchUpdateBuilder<M>; "User::update_all()", "q.where_eq(User::ACTIVE, value)");
conditional_methods!(
    impl[M: Model] crate::relations::EagerQueryBuilder<M>;
    "User::query().with_many(Vec::<&str>::new())",
    "q.query(|query| query.where_eq(User::ACTIVE, value))"
);

mod condition_value;
mod finite;
mod operator;
mod or_branch_builder;
mod or_group;
mod order;

pub use condition_value::ConditionValue;
pub(crate) use condition_value::{
    comparable_value, condition_is_vacuous, is_json_null, is_sql_null,
};
pub use operator::{LogicalOp, Operator};
pub use order::{Order, SortOrder};

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

/// A single where condition
#[derive(Debug, Clone)]
#[must_use = "a condition filters nothing until a query takes it"]
pub struct WhereCondition {
    /// The column, as written into the SQL (qualified when it names another
    /// table); empty for a raw condition.
    pub column: String,
    /// The comparison.
    pub operator: Operator,
    /// The operand.
    pub value: ConditionValue,
}
