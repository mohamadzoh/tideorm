use super::{ConditionValue, Operator, OrBranchBuilder, OrGroup, QueryBuilder, WhereCondition};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Add an OR group to the query.
    #[must_use]
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
    #[must_use]
    pub fn or_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_eq(column, value));
        self
    }

    #[must_use]
    pub fn or_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_not(column, value));
        self
    }

    #[must_use]
    pub fn or_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_gt(column, value));
        self
    }

    #[must_use]
    pub fn or_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_gte(column, value));
        self
    }

    #[must_use]
    pub fn or_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_lt(column, value));
        self
    }

    #[must_use]
    pub fn or_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_lte(column, value));
        self
    }

    #[must_use]
    pub fn or_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.or_groups
            .push(OrGroup::new().where_like(column, pattern));
        self
    }

    #[must_use]
    pub fn or_where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.or_groups
            .push(OrGroup::new().where_contains(column, value));
        self
    }

    #[must_use]
    pub fn or_where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.or_groups
            .push(OrGroup::new().where_starts_with(column, value));
        self
    }

    #[must_use]
    pub fn or_where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.or_groups
            .push(OrGroup::new().where_ends_with(column, value));
        self
    }

    #[must_use]
    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.or_groups.push(OrGroup::new().where_in(column, values));
        self
    }

    #[must_use]
    pub fn or_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.or_groups
            .push(OrGroup::new().where_not_in(column, values));
        self
    }

    #[must_use]
    pub fn or_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.or_groups.push(OrGroup::new().where_null(column));
        self
    }

    #[must_use]
    pub fn or_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.or_groups.push(OrGroup::new().where_not_null(column));
        self
    }

    #[must_use]
    pub fn or_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.or_groups
            .push(OrGroup::new().where_between(column, min, max));
        self
    }

    #[must_use]
    pub fn or_where_raw(mut self, raw_sql: &str) -> Self {
        self.or_groups.push(OrGroup::new().where_raw(raw_sql));
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
    #[must_use]
    pub fn eq_any<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::EqAny,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a WHERE column <> ALL(array) condition.
    #[must_use]
    pub fn ne_all<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NeAll,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a WHERE condition using a strongly-typed column.
    #[must_use]
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
                        let mut iter = arr.into_iter();
                        ConditionValue::Range(
                            iter.next().unwrap_or(serde_json::Value::Null),
                            iter.next().unwrap_or(serde_json::Value::Null),
                        )
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
