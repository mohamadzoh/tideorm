#![allow(missing_docs)]

use super::QueryBuilder;
use crate::model::Model;

mod or_branch;
mod or_branch_builder;
mod or_group;

pub use or_branch::OrBranch;
pub use or_branch_builder::OrBranchBuilder;
pub use or_group::OrGroup;

/// Sort order for queries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Ascending order (A-Z, 1-9)
    Asc,
    /// Descending order (Z-A, 9-1)
    Desc,
}

impl Order {
    /// Convert to SQL string
    pub fn as_str(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

/// Comparison operators for where clauses
#[derive(Debug, Clone)]
pub enum Operator {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    LikeEscaped,
    NotLike,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
    JsonContains,
    JsonContainedBy,
    JsonKeyExists,
    JsonKeyNotExists,
    JsonPathExists,
    JsonPathNotExists,
    ArrayContains,
    ArrayContainedBy,
    ArrayOverlaps,
    ArrayContainsAny,
    ArrayContainsAll,
    SubqueryIn,
    SubqueryNotIn,
    Raw,
    EqAny,
    NeAll,
}

/// A single where condition
#[derive(Debug, Clone)]
pub struct WhereCondition {
    pub column: String,
    pub operator: Operator,
    pub value: ConditionValue,
}

/// Value for a where condition
#[derive(Debug, Clone)]
pub enum ConditionValue {
    Single(serde_json::Value),
    List(Vec<serde_json::Value>),
    Range(serde_json::Value, serde_json::Value),
    None,
    Subquery(String),
    RawExpr(String),
}

/// Logical operator for combining conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}

impl LogicalOp {
    pub fn as_sql(&self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
    }
}
