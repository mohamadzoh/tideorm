use super::{ConditionValue, Operator, QueryBuilder, WhereCondition};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Add a where equals condition
    ///
    /// Accepts either a string column name or a typed column reference.
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

    /// Add a where not equals condition
    ///
    /// Accepts either a string column name or a typed column reference.
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

    /// Add a where greater than condition
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

    /// Add a where greater than or equal condition
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

    /// Add a where less than condition
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

    /// Add a where less than or equal condition
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

    /// Add a where LIKE condition
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

    /// Add a where LIKE '%value%' condition using literal matching semantics.
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

    /// Add a where LIKE 'value%' condition using literal matching semantics.
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

    /// Add a where LIKE '%value' condition using literal matching semantics.
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

    /// Add a where NOT LIKE condition
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

    /// Add a where IN condition
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

    /// Add a where NOT IN condition
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
}
