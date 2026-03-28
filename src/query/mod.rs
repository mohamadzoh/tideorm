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
mod conditions;
mod db_sql;
mod filters;
#[allow(missing_docs)]
mod or_clauses;
mod predicates;
mod sql;
mod structure;

pub use filters::{
    ConditionValue, LogicalOp, Operator, OrBranch, OrBranchBuilder, OrGroup, Order, WhereCondition,
};
pub use structure::{
    AggregateFunction, CTE, FrameBound, FrameType, JoinClause, JoinResultConsolidator, JoinType,
    QueryFragment, UnionClause, UnionType, WindowFunction, WindowFunctionType,
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
    order_by: Vec<(String, Order)>,
    limit_value: Option<u64>,
    offset_value: Option<u64>,
    select_columns: Option<Vec<String>>,
    raw_select_expressions: Vec<String>,
    include_trashed: bool,
    only_trashed: bool,
    joins: Vec<JoinClause>,
    invalid_query_reason: Option<String>,
    group_by: Vec<String>,
    having_conditions: Vec<String>,
    unions: Vec<UnionClause>,
    window_functions: Vec<WindowFunction>,
    ctes: Vec<CTE>,
    cache_options: Option<crate::cache::CacheOptions>,
    cache_key: Option<String>,
}

impl<M: Model> QueryBuilder<M> {
    /// Rebuild a query builder from a reusable fragment.
    pub fn from_fragment(fragment: &QueryFragment<M>) -> Self {
        Self::new().apply(fragment)
    }
}

#[cfg(test)]
#[path = "../testing/query_tests.rs"]
mod tests;
