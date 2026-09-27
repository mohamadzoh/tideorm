//! Fluent query builder
//!
//! This module contains the query builder, its condition types, and the SQL
//! rendering and execution pipeline behind model reads and bulk mutations.
//!
//! When a query behaves unexpectedly, the fastest path is usually:
//! - inspect the builder with `debug()`
//! - check whether the builder was invalidated before execution
//! - inspect raw SQL validation errors for unsafe fragments or invalid subqueries
//!
//! Use the fluent builder for normal reads and mutations. Drop to raw SQL only
//! when the builder cannot express the query shape you need, and expect those
//! paths to fail validation earlier if the fragment is unsafe or not shaped like
//! the API expects.

use std::marker::PhantomData;

use crate::model::Model;

mod advanced;
mod builder;
pub(crate) mod db_sql;
mod filters;
mod or_clauses;
mod predicates;
mod sql;
mod structure;

pub(crate) use advanced::page_offset;
pub use advanced::{Aggregate, AggregateCondition, HavingCondition};
pub use filters::{
    ConditionValue, LogicalOp, Operator, OrBranchBuilder, OrGroup, Order, SortOrder, WhereCondition,
};
pub(crate) use filters::{
    checked_filter_value, condition_is_vacuous, condition_methods, filter_value,
};
pub use sql::execution::Paginated;
pub use structure::{
    CTE, FrameBound, FrameType, JoinClause, JoinResultConsolidator, JoinType, QueryFragment,
    UnionClause, UnionType, WindowFunction, WindowFunctionType,
};

/// Fluent query builder for TideORM models.
#[derive(Debug, Clone)]
pub struct QueryBuilder<M: Model> {
    _marker: PhantomData<M>,
    database: Option<crate::database::Database>,
    /// WHERE conditions combined with AND logic.
    pub conditions: Vec<WhereCondition>,
    /// OR groups for complex boolean expressions.
    pub or_groups: Vec<OrGroup>,
    /// Index into `or_groups` of the group the `or_where_*` calls share.
    simple_or_group: Option<usize>,
    order_by: Vec<(String, Order)>,
    limit_value: Option<u64>,
    offset_value: Option<u64>,
    select_columns: Option<Vec<String>>,
    raw_select_expressions: Vec<String>,
    subquery_select_expressions: Vec<structure::SubquerySelect>,
    include_trashed: bool,
    only_trashed: bool,
    lock_for_update: bool,
    joins: Vec<JoinClause>,
    invalid_query_reason: Option<String>,
    /// A page number or size `page()` refused, as the field it names and why:
    /// reported as the validation error `Model::paginate` gives the same input.
    invalid_page: Option<(&'static str, String)>,
    group_by: Vec<String>,
    having_conditions: Vec<String>,
    having_bindings: Vec<Vec<crate::internal::Value>>,
    unions: Vec<UnionClause>,
    window_functions: Vec<WindowFunction>,
    ctes: Vec<CTE>,
    cache_options: Option<crate::cache::CacheOptions>,
    cache_key: Option<String>,
    /// Column-type lookups for models whose tables the query joins, consulted
    /// after `M`'s own when a filter value is bound.
    joined_column_types: Vec<fn(&str) -> Option<crate::orm::ColumnType>>,
    /// How many `where_has` subqueries over this query's own table enclose it.
    /// Above zero the query reads its table under an alias of its own, so a
    /// qualified column or a nested correlation names this query's row and
    /// not an enclosing one's.
    self_join_depth: usize,
}

impl<M: Model> QueryBuilder<M> {
    /// Rebuild a query builder from a reusable fragment.
    #[must_use]
    pub fn from_fragment(fragment: &QueryFragment<M>) -> Self {
        Self::new().apply(fragment)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/query_tests.rs"]
mod tests;
