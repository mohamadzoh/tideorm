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

impl ComparisonOperator {
    fn of(operator: &Operator) -> Option<Self> {
        Some(match operator {
            Operator::Eq => Self::Eq,
            Operator::NotEq => Self::NotEq,
            Operator::Gt => Self::Gt,
            Operator::Gte => Self::Gte,
            Operator::Lt => Self::Lt,
            Operator::Lte => Self::Lte,
            _ => return None,
        })
    }

    /// `left <operator> right`.
    fn apply(self, left: SimpleExpr, right: impl Into<SimpleExpr>) -> SimpleExpr {
        match self {
            Self::Eq => left.eq(right),
            Self::NotEq => left.ne(right),
            Self::Gt => left.gt(right),
            Self::Gte => left.gte(right),
            Self::Lt => left.lt(right),
            Self::Lte => left.lte(right),
        }
    }
}

#[derive(Clone, Copy)]
enum ListOperator {
    In,
    NotIn,
}

impl ListOperator {
    fn of(operator: &Operator) -> Option<Self> {
        match operator {
            Operator::In => Some(Self::In),
            Operator::NotIn => Some(Self::NotIn),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum ArrayOperator {
    Contains,
    ContainedBy,
    Overlaps,
}

impl ArrayOperator {
    fn of(operator: &Operator) -> Option<Self> {
        match operator {
            Operator::ArrayContains => Some(Self::Contains),
            Operator::ArrayContainedBy => Some(Self::ContainedBy),
            Operator::ArrayOverlaps => Some(Self::Overlaps),
            _ => None,
        }
    }
}

/// The rendering a condition's operator/value pair calls for.
///
/// `condition_spec` returns `None` for a pair with no rendering, which is what
/// lets execution reject such a condition instead of silently dropping it.
enum ConditionSpec<'a> {
    Raw {
        raw_sql: &'a str,
        values: &'a [Value],
        /// Whether `raw_sql` is a caller's template, whose `?`s become the
        /// backend's markers at rendering.
        template: bool,
    },
    Compare {
        operator: ComparisonOperator,
        value: &'a serde_json::Value,
    },
    /// The column compared with another column of the same row.
    CompareColumns {
        operator: ComparisonOperator,
        other: &'a str,
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
        negated: bool,
    },
    JsonValue {
        containment: db_sql::JsonContainment,
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
