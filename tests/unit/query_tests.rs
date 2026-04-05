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

#[path = "query_tests/db_sql_and_safety_tests.rs"]
mod db_sql_and_safety_tests;

#[path = "query_tests/validation_and_async_tests.rs"]
mod validation_and_async_tests;

#[path = "query_tests/window_and_cte_tests.rs"]
mod window_and_cte_tests;

#[path = "query_tests/query_builder_sql_tests.rs"]
mod query_builder_sql_tests;
