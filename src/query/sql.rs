use super::*;
use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{Alias, Condition, Expr, ExprTrait, SimpleExpr, Value};
use crate::model::Model;
use crate::orm::sql_builder::{
    ArrayOperator, BoundSql, JsonContainment, JsonExistence, error::RenderError,
};
use crate::soft_delete::{SoftDeleteScope, query_scope_for};

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

/// The array predicate `operator` names, if it names one.
fn array_operator(operator: &Operator) -> Option<ArrayOperator> {
    match operator {
        Operator::ArrayContains => Some(ArrayOperator::Contains),
        Operator::ArrayContainedBy => Some(ArrayOperator::ContainedBy),
        Operator::ArrayOverlaps => Some(ArrayOperator::Overlaps),
        _ => None,
    }
}

/// The rendering a condition's operator/value pair calls for.
///
/// `condition_spec` returns `None` for a pair with no rendering, which is what
/// lets execution reject such a condition instead of silently dropping it.
enum ConditionSpec<'a> {
    /// Trusted SQL, whose `?` placeholders bind `values` when it has any.
    Raw {
        raw_sql: &'a str,
        values: &'a [Value],
    },
    /// A query the condition embeds.
    Subquery(&'a crate::query::Subquery),
    Compare {
        operator: ComparisonOperator,
        value: &'a Value,
    },
    /// The column compared with another column of the same row.
    CompareColumns {
        operator: ComparisonOperator,
        other: &'a str,
    },
    Pattern {
        negated: bool,
        value: &'a Value,
    },
    /// The text holds `value` literally, anything before it when `leading`
    /// and after it when `trailing`.
    Literal {
        leading: bool,
        trailing: bool,
        value: &'a Value,
    },
    List {
        operator: ListOperator,
        values: &'a [Value],
    },
    NullCheck {
        negated: bool,
    },
    Between {
        low: &'a Value,
        high: &'a Value,
        negated: bool,
    },
    JsonValue {
        containment: JsonContainment,
        value: &'a Value,
    },
    JsonExists {
        existence: JsonExistence,
        negated: bool,
        target: &'a str,
    },
    Array {
        operator: ArrayOperator,
        values: &'a [Value],
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
