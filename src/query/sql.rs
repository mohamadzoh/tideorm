use super::*;
use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{Condition, Expr, ExprTrait, Value};
use crate::model::Model;
use crate::soft_delete::{SoftDeleteScope, query_scope_for};
use sea_orm::sea_query::{
    Alias, MysqlQueryBuilder, PostgresQueryBuilder, Query, SimpleExpr, SqliteQueryBuilder,
    extension::postgres::PgBinOper,
};

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
enum JsonStringOperator {
    KeyPresent,
    KeyAbsent,
    PathPresent,
    PathAbsent,
}

#[derive(Clone, Copy)]
enum ArrayOperator {
    Contains,
    ContainedBy,
    Overlaps,
}

enum ConditionSpec<'a> {
    Raw {
        column: &'a str,
        raw_sql: &'a str,
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
    JsonString {
        operator: JsonStringOperator,
        value: &'a str,
    },
    Array {
        operator: ArrayOperator,
        values: &'a [serde_json::Value],
    },
    Subquery {
        negated: bool,
        query_sql: &'a str,
    },
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../testing/query_sql_tests.rs"]
mod tests;
