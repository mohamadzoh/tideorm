use super::{ConditionValue, Operator, OrBranchBuilder, OrGroup, QueryBuilder, WhereCondition};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Add an OR group to the query.
    pub fn or_where<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let group = f(OrGroup::new());
        if !group.is_empty() {
            self.or_groups.push(group);
        }
        self
    }

    /// Add an OR condition directly (simple shorthand).
    pub fn or_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotEq,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gt,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gte,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lt,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lte,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Like,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::LikeEscaped,
            value: ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotIn,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(min.into(), max.into()),
        });
        self.or_groups.push(group);
        self
    }

    pub fn or_where_raw(mut self, raw_sql: &str) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self.or_groups.push(group);
        self
    }

    /// Start building a fluent OR expression with chained AND conditions.
    pub fn begin_or(self) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self)
    }

    pub fn begin_or_where_eq(
        self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_eq(column, value)
    }

    pub fn begin_or_where_gt(
        self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_gt(column, value)
    }

    pub fn begin_or_where_gte(
        self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_gte(column, value)
    }

    pub fn begin_or_where_lt(
        self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_lt(column, value)
    }

    pub fn begin_or_where_lte(
        self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_lte(column, value)
    }

    pub fn begin_or_where_like(
        self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_like(column, pattern)
    }

    pub fn begin_or_where_contains(
        self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_contains(column, value)
    }

    pub fn begin_or_where_starts_with(
        self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_starts_with(column, value)
    }

    pub fn begin_or_where_ends_with(
        self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_ends_with(column, value)
    }

    pub fn begin_or_where_in<V: Into<serde_json::Value>>(
        self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_in(column, values)
    }

    pub fn begin_or_where_null(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_null(column)
    }

    pub fn begin_or_where_not_null(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_not_null(column)
    }

    pub fn begin_or_where_between(
        self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_between(column, min, max)
    }

    /// Add a WHERE column = ANY(array) condition (PostgreSQL optimization).
    pub fn eq_any<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::EqAny,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a WHERE column <> ALL(array) condition.
    pub fn ne_all<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NeAll,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a WHERE condition using a strongly-typed column.
    pub fn where_col(mut self, condition: crate::columns::ColumnCondition) -> Self {
        let operator = match condition.operator {
            crate::columns::ColumnOperator::Eq => Operator::Eq,
            crate::columns::ColumnOperator::NotEq => Operator::NotEq,
            crate::columns::ColumnOperator::Gt => Operator::Gt,
            crate::columns::ColumnOperator::Gte => Operator::Gte,
            crate::columns::ColumnOperator::Lt => Operator::Lt,
            crate::columns::ColumnOperator::Lte => Operator::Lte,
            crate::columns::ColumnOperator::Like => Operator::Like,
            crate::columns::ColumnOperator::LikeEscaped => Operator::LikeEscaped,
            crate::columns::ColumnOperator::NotLike => Operator::NotLike,
            crate::columns::ColumnOperator::In => Operator::In,
            crate::columns::ColumnOperator::NotIn => Operator::NotIn,
            crate::columns::ColumnOperator::IsNull => Operator::IsNull,
            crate::columns::ColumnOperator::IsNotNull => Operator::IsNotNull,
            crate::columns::ColumnOperator::Between => Operator::Between,
        };

        let value = match condition.operator {
            crate::columns::ColumnOperator::IsNull | crate::columns::ColumnOperator::IsNotNull => {
                ConditionValue::None
            }
            crate::columns::ColumnOperator::In | crate::columns::ColumnOperator::NotIn => {
                if let serde_json::Value::Array(arr) = condition.value {
                    ConditionValue::List(arr)
                } else {
                    ConditionValue::List(vec![condition.value])
                }
            }
            crate::columns::ColumnOperator::Between => {
                if let serde_json::Value::Array(arr) = condition.value {
                    if arr.len() >= 2 {
                        ConditionValue::Range(arr[0].clone(), arr[1].clone())
                    } else {
                        ConditionValue::Single(serde_json::Value::Null)
                    }
                } else {
                    ConditionValue::Single(condition.value)
                }
            }
            _ => ConditionValue::Single(condition.value),
        };

        self.conditions.push(WhereCondition {
            column: condition.column,
            operator,
            value,
        });
        self
    }
}
