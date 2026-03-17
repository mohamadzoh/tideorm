#![allow(missing_docs)]

use crate::error::{Error, Result};

use super::Model;

/// Builder for batch update operations.
pub struct BatchUpdateBuilder<M: Model> {
    _marker: std::marker::PhantomData<M>,
    updates: std::collections::HashMap<String, UpdateValue>,
    conditions: Vec<crate::query::WhereCondition>,
    returning: bool,
    limit_value: Option<u64>,
}

/// Value for batch update operations.
#[derive(Debug, Clone)]
pub enum UpdateValue {
    Value(serde_json::Value),
    Raw(String),
    Increment(i64),
    Decrement(i64),
    Multiply(f64),
    Divide(f64),
    ArrayAppend(serde_json::Value),
    ArrayRemove(serde_json::Value),
    JsonSet(String, serde_json::Value),
    Coalesce(serde_json::Value),
}

impl<M: Model> BatchUpdateBuilder<M> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            updates: std::collections::HashMap::new(),
            conditions: Vec::new(),
            returning: false,
            limit_value: None,
        }
    }

    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Value(value.into()));
        self
    }

    pub fn set_raw(mut self, field: &str, expression: &str) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Raw(expression.to_string()));
        self
    }

    pub fn set_if<F>(
        mut self,
        field: &str,
        value: impl Into<serde_json::Value>,
        condition: F,
    ) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        self.updates
            .insert(field.to_string(), UpdateValue::Value(value.into()));
        condition(self)
    }

    pub fn increment(mut self, field: &str, by: i64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Increment(by));
        self
    }

    pub fn decrement(mut self, field: &str, by: i64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Decrement(by));
        self
    }

    pub fn multiply(mut self, field: &str, by: f64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Multiply(by));
        self
    }

    pub fn divide(mut self, field: &str, by: f64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Divide(by));
        self
    }

    pub fn array_append(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::ArrayAppend(value.into()));
        self
    }

    pub fn array_remove(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::ArrayRemove(value.into()));
        self
    }

    pub fn json_set(
        mut self,
        field: &str,
        path: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.to_string(),
            UpdateValue::JsonSet(path.to_string(), value.into()),
        );
        self
    }

    pub fn coalesce(mut self, field: &str, default: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Coalesce(default.into()));
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    pub fn returning(mut self) -> Self {
        self.returning = true;
        self
    }

    pub fn where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_not(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gte(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Gte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lte(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Lte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::NotIn,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    pub fn where_null(mut self, column: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    pub fn where_not_null(mut self, column: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::IsNotNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    pub fn where_between(
        mut self,
        column: &str,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Between,
            value: crate::query::ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    pub fn where_like(mut self, column: &str, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    pub fn or_where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_not(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_gt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_lt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    pub fn or_where_null(mut self, column: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    pub fn or_where_like(mut self, column: &str, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    fn format_value(v: &serde_json::Value) -> String {
        match v {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
            _ => format!("'{}'", v.to_string().replace('\'', "''")),
        }
    }

    pub async fn execute(self) -> Result<u64> {
        if self.updates.is_empty() {
            return Ok(0);
        }

        let _ = self.returning;

        let db_type = crate::database::require_db()?.backend();
        let quote = match db_type {
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => '`',
            _ => '"',
        };

        let set_parts: Vec<String> = self
            .updates
            .iter()
            .map(|(k, v)| {
                let col = format!("{0}{1}{0}", quote, k);
                match v {
                    UpdateValue::Value(val) => {
                        format!("{} = {}", col, Self::format_value(val))
                    }
                    UpdateValue::Raw(expr) => {
                        format!("{} = {}", col, expr)
                    }
                    UpdateValue::Increment(by) => {
                        format!("{} = {} + {}", col, col, by)
                    }
                    UpdateValue::Decrement(by) => {
                        format!("{} = {} - {}", col, col, by)
                    }
                    UpdateValue::Multiply(by) => {
                        format!("{} = {} * {}", col, col, by)
                    }
                    UpdateValue::Divide(by) => {
                        format!("{} = {} / {}", col, col, by)
                    }
                    UpdateValue::ArrayAppend(val) => match db_type {
                        crate::config::DatabaseType::Postgres => {
                            format!("{} = array_append({}, {})", col, col, Self::format_value(val))
                        }
                        crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                            format!("{} = JSON_ARRAY_APPEND({}, '$', {})", col, col, Self::format_value(val))
                        }
                        crate::config::DatabaseType::SQLite => {
                            format!("{} = json_insert({}, '$[#]', {})", col, col, Self::format_value(val))
                        }
                    },
                    UpdateValue::ArrayRemove(val) => match db_type {
                        crate::config::DatabaseType::Postgres => {
                            format!("{} = array_remove({}, {})", col, col, Self::format_value(val))
                        }
                        crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                            format!("{} = JSON_REMOVE({}, JSON_UNQUOTE(JSON_SEARCH({}, 'one', {})))", col, col, col, Self::format_value(val))
                        }
                        crate::config::DatabaseType::SQLite => {
                            format!("{} = (SELECT json_group_array(value) FROM json_each({}) WHERE value != {})", col, col, Self::format_value(val))
                        }
                    },
                    UpdateValue::JsonSet(path, val) => match db_type {
                        crate::config::DatabaseType::Postgres => {
                            format!("{} = jsonb_set({}, '{{{}}}', '{}')", col, col, path.trim_start_matches("$."), Self::format_value(val).trim_matches('\''))
                        }
                        crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                            format!("{} = JSON_SET({}, '{}', {})", col, col, path, Self::format_value(val))
                        }
                        crate::config::DatabaseType::SQLite => {
                            format!("{} = json_set({}, '{}', {})", col, col, path, Self::format_value(val))
                        }
                    },
                    UpdateValue::Coalesce(default) => {
                        format!("{} = COALESCE({}, {})", col, col, Self::format_value(default))
                    }
                }
            })
            .collect();

        let mut and_parts: Vec<String> = Vec::new();
        let mut or_parts: Vec<String> = Vec::new();

        for cond in &self.conditions {
            let (is_or, actual_column) = if cond.column.starts_with("__OR__") {
                (
                    true,
                    cond.column.strip_prefix("__OR__").unwrap_or(&cond.column),
                )
            } else {
                (false, cond.column.as_str())
            };

            let col = format!("{0}{1}{0}", quote, actual_column);

            let part = match &cond.operator {
                crate::query::Operator::Eq => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} = {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::NotEq => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} != {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Gt => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} > {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Gte => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} >= {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Lt => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} < {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Lte => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} <= {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::In => {
                    if let crate::query::ConditionValue::List(vals) = &cond.value {
                        let list = vals
                            .iter()
                            .map(Self::format_value)
                            .collect::<Vec<_>>()
                            .join(", ");
                        Some(format!("{} IN ({})", col, list))
                    } else {
                        None
                    }
                }
                crate::query::Operator::NotIn => {
                    if let crate::query::ConditionValue::List(vals) = &cond.value {
                        let list = vals
                            .iter()
                            .map(Self::format_value)
                            .collect::<Vec<_>>()
                            .join(", ");
                        Some(format!("{} NOT IN ({})", col, list))
                    } else {
                        None
                    }
                }
                crate::query::Operator::IsNull => Some(format!("{} IS NULL", col)),
                crate::query::Operator::IsNotNull => Some(format!("{} IS NOT NULL", col)),
                crate::query::Operator::Between => {
                    if let crate::query::ConditionValue::Range(min, max) = &cond.value {
                        Some(format!(
                            "{} BETWEEN {} AND {}",
                            col,
                            Self::format_value(min),
                            Self::format_value(max)
                        ))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Like => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} LIKE {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(part) = part {
                if is_or {
                    or_parts.push(part);
                } else {
                    and_parts.push(part);
                }
            }
        }

        let table = format!("{0}{1}{0}", quote, M::table_name());
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !and_parts.is_empty() || !or_parts.is_empty() {
            sql.push_str(" WHERE ");

            if and_parts.is_empty() {
                sql.push_str(&or_parts.join(" OR "));
            } else if or_parts.is_empty() {
                sql.push_str(&and_parts.join(" AND "));
            } else {
                sql.push_str(&format!(
                    "{} AND ({})",
                    and_parts.join(" AND "),
                    or_parts.join(" OR ")
                ));
            }
        }

        if let Some(limit) = self.limit_value {
            if matches!(
                db_type,
                crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
            ) {
                sql.push_str(&format!(" LIMIT {}", limit));
            }
        }

        crate::Database::execute(&sql).await
    }

    pub async fn execute_returning(self) -> Result<Vec<M>> {
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        let db_type = crate::database::require_db()?.backend();

        if db_type == crate::config::DatabaseType::MySQL {
            return Err(Error::query(
                "MySQL does not support RETURNING clause".to_string(),
            ));
        }

        let quote = match db_type {
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => '`',
            _ => '"',
        };

        let set_parts: Vec<String> = self
            .updates
            .iter()
            .map(|(k, v)| {
                let col = format!("{0}{1}{0}", quote, k);
                match v {
                    UpdateValue::Value(val) => format!("{} = {}", col, Self::format_value(val)),
                    UpdateValue::Raw(expr) => format!("{} = {}", col, expr),
                    UpdateValue::Increment(by) => format!("{} = {} + {}", col, col, by),
                    UpdateValue::Decrement(by) => format!("{} = {} - {}", col, col, by),
                    UpdateValue::Multiply(by) => format!("{} = {} * {}", col, col, by),
                    UpdateValue::Divide(by) => format!("{} = {} / {}", col, col, by),
                    _ => format!("{} = {}", col, col),
                }
            })
            .collect();

        let where_parts: Vec<String> = self
            .conditions
            .iter()
            .filter_map(|cond| {
                let col = format!("{0}{1}{0}", quote, cond.column);
                match &cond.operator {
                    crate::query::Operator::Eq => {
                        if let crate::query::ConditionValue::Single(v) = &cond.value {
                            Some(format!("{} = {}", col, Self::format_value(v)))
                        } else {
                            None
                        }
                    }
                    crate::query::Operator::NotEq => {
                        if let crate::query::ConditionValue::Single(v) = &cond.value {
                            Some(format!("{} != {}", col, Self::format_value(v)))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            })
            .collect();

        let table = format!("{0}{1}{0}", quote, M::table_name());
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
        }

        sql.push_str(" RETURNING *");

        crate::Database::raw::<M>(&sql).await
    }
}

impl<M: Model> Default for BatchUpdateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}