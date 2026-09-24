use super::*;
use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{
    Alias, Condition, Expr, ExprTrait, MysqlQueryBuilder, PostgresQueryBuilder, Query, SimpleExpr,
    SqliteQueryBuilder, Value,
};
use crate::model::Model;
use crate::soft_delete::{SoftDeleteScope, query_scope_for};
use db_sql::JsonExistence;

pub(crate) mod condition_builders;
pub(crate) mod debugging;
pub(crate) mod execution;
pub(crate) mod rendering;

#[derive(Clone, Copy)]
enum ComparisonOperator {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Clone, Copy)]
enum ListOperator {
    In,
    NotIn,
    EqAny,
    NeAll,
}

#[derive(Clone, Copy)]
enum JsonValueOperator {
    Contains,
    ContainedBy,
}

#[derive(Clone, Copy)]
enum ArrayOperator {
    Contains,
    ContainedBy,
    Overlaps,
}

/// The rendering a condition's operator/value pair calls for.
///
/// `condition_spec` returns `None` for a pair with no rendering, which is what
/// lets execution reject such a condition instead of silently dropping it.
enum ConditionSpec<'a> {
    Raw {
        raw_sql: &'a str,
        values: &'a [Value],
    },
    Compare {
        operator: ComparisonOperator,
        value: &'a serde_json::Value,
    },
    Pattern {
        negated: bool,
        escaped: bool,
        value: &'a serde_json::Value,
    },
    List {
        operator: ListOperator,
        values: &'a [serde_json::Value],
    },
    NullCheck {
        negated: bool,
    },
    Between {
        low: &'a serde_json::Value,
        high: &'a serde_json::Value,
    },
    JsonValue {
        operator: JsonValueOperator,
        value: &'a serde_json::Value,
    },
    JsonExists {
        existence: JsonExistence,
        negated: bool,
        target: &'a str,
    },
    Array {
        operator: ArrayOperator,
        values: &'a [serde_json::Value],
    },
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/query_sql_tests.rs"]
mod tests;
