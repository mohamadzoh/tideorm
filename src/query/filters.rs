#![allow(missing_docs)]

use super::QueryBuilder;
use crate::model::Model;

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

/// A group of conditions combined with a logical operator
#[derive(Debug, Clone)]
pub struct OrGroup {
    pub conditions: Vec<WhereCondition>,
    pub nested_groups: Vec<OrGroup>,
    pub combine_with: LogicalOp,
}

impl OrGroup {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            nested_groups: Vec::new(),
            combine_with: LogicalOp::Or,
        }
    }

    pub fn where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotEq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Like,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }

    pub fn where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_not_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotLike,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }

    pub fn where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotIn,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    pub fn where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    pub fn nested_or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let nested = f(OrGroup::new());
        self.nested_groups.push(nested);
        self
    }

    pub fn nested_and<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let mut nested = OrGroup::new();
        nested.combine_with = LogicalOp::And;
        nested = f(nested);
        self.nested_groups.push(nested);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.nested_groups.is_empty()
    }

    pub fn condition_count(&self) -> usize {
        let nested_count: usize = self.nested_groups.iter().map(|g| g.condition_count()).sum();
        self.conditions.len() + nested_count
    }
}

impl Default for OrGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// A single branch in an OR expression
#[derive(Debug, Clone)]
pub struct OrBranch {
    pub conditions: Vec<WhereCondition>,
}

impl OrBranch {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotEq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Like,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }

    pub fn where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_not_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotLike,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }

    pub fn where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotIn,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    pub fn where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.conditions.len()
    }
}

impl Default for OrBranch {
    fn default() -> Self {
        Self::new()
    }
}

/// Fluent builder for chained OR conditions
#[derive(Debug)]
pub struct OrBranchBuilder<M: Model> {
    query: QueryBuilder<M>,
    branches: Vec<OrBranch>,
    current_branch: OrBranch,
}

impl<M: Model> OrBranchBuilder<M> {
    pub fn new(query: QueryBuilder<M>) -> Self {
        Self {
            query,
            branches: Vec::new(),
            current_branch: OrBranch::new(),
        }
    }

    pub fn or_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_eq(column, value);
        self
    }

    pub fn or_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not(column, value);
        self
    }

    pub fn or_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gt(column, value);
        self
    }

    pub fn or_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gte(column, value);
        self
    }

    pub fn or_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lt(column, value);
        self
    }

    pub fn or_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lte(column, value);
        self
    }

    pub fn or_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_like(column, pattern);
        self
    }

    pub fn or_where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_contains(column, value);
        self
    }

    pub fn or_where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_starts_with(column, value);
        self
    }

    pub fn or_where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_ends_with(column, value);
        self
    }

    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_in(column, values);
        self
    }

    pub fn or_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not_in(column, values);
        self
    }

    pub fn or_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_null(column);
        self
    }

    pub fn or_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not_null(column);
        self
    }

    pub fn or_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_between(column, min, max);
        self
    }

    pub fn or_where_raw(mut self, raw_sql: &str) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_raw(raw_sql);
        self
    }

    pub fn and_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_eq(column, value);
        self
    }

    pub fn and_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_not(column, value);
        self
    }

    pub fn and_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_gt(column, value);
        self
    }

    pub fn and_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_gte(column, value);
        self
    }

    pub fn and_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_lt(column, value);
        self
    }

    pub fn and_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_lte(column, value);
        self
    }

    pub fn and_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_like(column, pattern);
        self
    }

    pub fn and_where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_contains(column, value);
        self
    }

    pub fn and_where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_starts_with(column, value);
        self
    }

    pub fn and_where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_ends_with(column, value);
        self
    }

    pub fn and_where_not_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_not_like(column, pattern);
        self
    }

    pub fn and_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.current_branch = self.current_branch.where_in(column, values);
        self
    }

    pub fn and_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.current_branch = self.current_branch.where_not_in(column, values);
        self
    }

    pub fn and_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.current_branch = self.current_branch.where_null(column);
        self
    }

    pub fn and_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.current_branch = self.current_branch.where_not_null(column);
        self
    }

    pub fn and_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_between(column, min, max);
        self
    }

    pub fn and_where_raw(mut self, raw_sql: &str) -> Self {
        self.current_branch = self.current_branch.where_raw(raw_sql);
        self
    }

    pub fn end_or(mut self) -> QueryBuilder<M> {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }

        if !self.branches.is_empty() {
            let mut or_group = OrGroup::new();

            for branch in self.branches {
                if branch.conditions.len() == 1 {
                    if let Some(condition) = branch.conditions.into_iter().next() {
                        or_group.conditions.push(condition);
                    }
                } else {
                    let mut nested = OrGroup::new();
                    nested.combine_with = LogicalOp::And;
                    nested.conditions = branch.conditions;
                    or_group.nested_groups.push(nested);
                }
            }

            self.query.or_groups.push(or_group);
        }

        self.query
    }

    pub fn branch_count(&self) -> usize {
        let current = if self.current_branch.is_empty() { 0 } else { 1 };
        self.branches.len() + current
    }

    pub fn total_conditions(&self) -> usize {
        let mut total: usize = self.branches.iter().map(|b| b.len()).sum();
        total += self.current_branch.len();
        total
    }
}
