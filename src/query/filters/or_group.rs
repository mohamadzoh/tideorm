use super::*;

/// A group of conditions combined with a logical operator
#[derive(Debug, Clone)]
pub struct OrGroup {
    pub conditions: Vec<WhereCondition>,
    pub nested_groups: Vec<OrGroup>,
    pub combine_with: LogicalOp,
}

impl OrGroup {
    #[must_use]
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            nested_groups: Vec::new(),
            combine_with: LogicalOp::Or,
        }
    }

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    #[must_use]
    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    #[must_use]
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

    #[must_use]
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    #[must_use]
    pub fn nested_or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let nested = f(OrGroup::new());
        self.nested_groups.push(nested);
        self
    }

    #[must_use]
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
