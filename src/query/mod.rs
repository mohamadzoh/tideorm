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
mod clauses;
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
    pub(crate) clauses: clauses::Clauses,
    /// How many `where_has` subqueries over this query's own table enclose it.
    /// Above zero the query reads its table under an alias of its own, so a
    /// qualified column or a nested correlation names this query's row and
    /// not an enclosing one's.
    self_join_depth: usize,
}

#[cfg(test)]
#[path = "../../tests/unit/query_tests.rs"]
mod tests;
