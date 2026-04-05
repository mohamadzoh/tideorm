use super::*;

impl<M: Model> BatchUpdateBuilder<M> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            updates: std::collections::HashMap::new(),
            conditions: Vec::new(),
            returning: false,
            limit_value: None,
        }
    }

    #[must_use]
    pub fn set(mut self, field: impl IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::Value(value.into()),
        );
        self
    }

    #[must_use]
    pub fn set_trusted_raw(mut self, field: impl IntoColumnName, expression: &str) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::UnsafeRaw(expression.to_string()),
        );
        self
    }

    #[must_use]
    pub fn set_if(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
        condition: bool,
    ) -> Self {
        if condition {
            self.updates.insert(
                field.column_name().to_string(),
                UpdateValue::Value(value.into()),
            );
        }
        self
    }

    #[must_use]
    pub fn increment(mut self, field: impl IntoColumnName, by: i64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Increment(by));
        self
    }

    #[must_use]
    pub fn decrement(mut self, field: impl IntoColumnName, by: i64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Decrement(by));
        self
    }

    #[must_use]
    pub fn multiply(mut self, field: impl IntoColumnName, by: f64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Multiply(by));
        self
    }

    #[must_use]
    pub fn divide(mut self, field: impl IntoColumnName, by: f64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Divide(by));
        self
    }

    #[must_use]
    pub fn array_append(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::ArrayAppend(value.into()),
        );
        self
    }

    #[must_use]
    pub fn array_remove(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::ArrayRemove(value.into()),
        );
        self
    }

    #[must_use]
    pub fn json_set(
        mut self,
        field: impl IntoColumnName,
        path: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::JsonSet(path.to_string(), value.into()),
        );
        self
    }

    #[must_use]
    pub fn coalesce(
        mut self,
        field: impl IntoColumnName,
        default: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::Coalesce(default.into()),
        );
        self
    }

    #[must_use]
    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    #[must_use]
    pub fn returning(mut self) -> Self {
        self.returning = true;
        self
    }

    #[must_use]
    pub fn where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn where_not(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn where_gt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn where_gte(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Gte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn where_lt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn where_lte(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Lte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    #[must_use]
    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::NotIn,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    #[must_use]
    pub fn where_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    #[must_use]
    pub fn where_not_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::IsNotNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    #[must_use]
    pub fn where_between(
        mut self,
        column: impl IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Between,
            value: crate::query::ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    #[must_use]
    pub fn where_like(mut self, column: impl IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    #[must_use]
    pub fn where_contains(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    #[must_use]
    pub fn where_starts_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    #[must_use]
    pub fn where_ends_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    #[must_use]
    pub fn or_where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn or_where_not(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn or_where_gt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn or_where_lt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    #[must_use]
    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    #[must_use]
    pub fn or_where_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    #[must_use]
    pub fn or_where_like(mut self, column: impl IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    #[must_use]
    pub fn or_where_contains(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    #[must_use]
    pub fn or_where_starts_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    #[must_use]
    pub fn or_where_ends_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }
}
