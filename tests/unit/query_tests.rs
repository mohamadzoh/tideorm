use super::db_sql;
use super::{
    CTE, FrameBound, FrameType, Order, QueryBuilder, UnionClause, UnionType, WindowFunction,
    WindowFunctionType,
};
use crate::columns::ColumnLike;
use crate::config::DatabaseType;
#[cfg(feature = "fulltext")]
use crate::fulltext::{FullTextSearchBuilder, SearchMode};
use crate::internal::Value;
use crate::model::Model as ModelTrait;
use std::time::Duration;

#[tideorm::model(table = "query_test_users")]
struct QueryTestUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "scoped_query_test_users")]
struct ScopedQueryTestUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    active: bool,
    verified_at: Option<String>,
    role: String,
}

#[tideorm::scopes]
impl ScopedQueryTestUser {
    pub fn active(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query.where_eq(Self::columns.active, true)
    }

    pub fn verified(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query.where_not_null(Self::columns.verified_at)
    }

    pub fn role(query: QueryBuilder<Self>, role: &str) -> QueryBuilder<Self> {
        query.where_eq(Self::columns.role, role)
    }

    /// A `mut` parameter, which a trait method without a body cannot declare.
    pub fn at_most(query: QueryBuilder<Self>, mut count: u64) -> QueryBuilder<Self> {
        count = count.max(1);
        query.limit(count)
    }

    /// `Self` in a parameter means the model, which in the generated trait
    /// is the query builder.
    pub fn same_role_as(query: QueryBuilder<Self>, other: &Self) -> QueryBuilder<Self> {
        query.where_eq(Self::columns.role, other.role.clone())
    }
}

#[path = "query_tests/db_sql_and_safety_tests.rs"]
mod db_sql_and_safety_tests;

#[path = "query_tests/validation_and_async_tests.rs"]
mod validation_and_async_tests;

#[path = "query_tests/window_and_cte_tests.rs"]
mod window_and_cte_tests;

#[path = "query_tests/query_builder_sql_tests.rs"]
mod query_builder_sql_tests;

#[path = "query_tests/scope_methods_tests.rs"]
mod scope_methods_tests;
