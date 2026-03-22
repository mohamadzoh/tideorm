use super::{ConditionValue, Operator, QueryBuilder, WhereCondition};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Add a WHERE IN (subquery) condition.
    pub fn where_in_subquery<N: Model>(mut self, column: &str, subquery: QueryBuilder<N>) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for where_in_subquery(): {}", err));
        }

        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::SubqueryIn,
            value: ConditionValue::Subquery(subquery_sql),
        });
        self
    }

    /// Add a WHERE NOT IN (subquery) condition.
    pub fn where_not_in_subquery<N: Model>(
        mut self,
        column: &str,
        subquery: QueryBuilder<N>,
    ) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!(
                "invalid subquery for where_not_in_subquery(): {}",
                err
            ));
        }

        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::SubqueryNotIn,
            value: ConditionValue::Subquery(subquery_sql),
        });
        self
    }

    /// Add a WHERE EXISTS (subquery) condition.
    pub fn where_exists<N: Model>(mut self, subquery: QueryBuilder<N>) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for where_exists(): {}", err));
        }

        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(format!("EXISTS ({})", subquery_sql)),
        });
        self
    }

    /// Add a WHERE NOT EXISTS (subquery) condition.
    pub fn where_not_exists<N: Model>(mut self, subquery: QueryBuilder<N>) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for where_not_exists(): {}", err));
        }

        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(format!("NOT EXISTS ({})", subquery_sql)),
        });
        self
    }

    /// Check if related records exist matching a condition.
    pub fn has_related(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<serde_json::Value>,
    ) -> Self {
        let table = M::table_name();
        let value = condition_value.into();
        let value_sql = match &value {
            serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "NULL".to_string(),
            _ => value.to_string(),
        };

        let exists_sql = format!(
            "EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\" AND \"{}\".\"{}\" = {})",
            related_table,
            related_table,
            foreign_key,
            table,
            local_key,
            related_table,
            condition_column,
            value_sql
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(exists_sql),
        });
        self
    }

    /// Check if related records do NOT exist matching a condition.
    pub fn has_no_related(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<serde_json::Value>,
    ) -> Self {
        let table = M::table_name();
        let value = condition_value.into();
        let value_sql = match &value {
            serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "NULL".to_string(),
            _ => value.to_string(),
        };

        let not_exists_sql = format!(
            "NOT EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\" AND \"{}\".\"{}\" = {})",
            related_table,
            related_table,
            foreign_key,
            table,
            local_key,
            related_table,
            condition_column,
            value_sql
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(not_exists_sql),
        });
        self
    }

    /// Check if any related records exist (without condition).
    pub fn has_any_related(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
    ) -> Self {
        let table = M::table_name();

        let exists_sql = format!(
            "EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\")",
            related_table, related_table, foreign_key, table, local_key
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(exists_sql),
        });
        self
    }

    /// Check if no related records exist.
    pub fn has_no_related_at_all(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
    ) -> Self {
        let table = M::table_name();

        let not_exists_sql = format!(
            "NOT EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\")",
            related_table, related_table, foreign_key, table, local_key
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(not_exists_sql),
        });
        self
    }

    /// Convert this query builder to a subquery SQL string.
    pub fn to_subquery_sql(&self) -> String {
        self.build_select_sql()
    }

    /// Add a raw WHERE condition.
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_raw_sql_fragment("WHERE raw SQL", raw_sql)
        {
            self.invalidate_query(reason);
        }

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    /// Add a raw WHERE condition with a column comparison.
    pub fn where_column_raw(mut self, column: &str, raw_expr: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_raw_sql_fragment("WHERE raw column expression", raw_expr)
        {
            self.invalidate_query(reason);
        }

        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_expr.to_string()),
        });
        self
    }

    /// Add a raw SELECT expression.
    pub fn select_raw(mut self, raw_select: &str) -> Self {
        self.raw_select_expressions.push(raw_select.to_string());
        self
    }

    /// Add a scalar subquery as a SELECT expression.
    pub fn select_subquery<N: Model>(mut self, subquery: QueryBuilder<N>, alias: &str) -> Self {
        let subquery_sql = subquery.to_subquery_sql();
        self.raw_select_expressions
            .push(format!("({}) AS \"{}\"", subquery_sql, alias));
        self
    }

    /// Add a where IS NULL condition.
    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where IS NOT NULL condition.
    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where BETWEEN condition.
    pub fn where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        low: impl Into<serde_json::Value>,
        high: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(low.into(), high.into()),
        });
        self
    }

    /// Add a JSON contains condition (column @> value).
    pub fn where_json_contains(
        mut self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonContains,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a JSON contained by condition (column <@ value).
    pub fn where_json_contained_by(
        mut self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonContainedBy,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a JSON key exists condition (column ? key).
    pub fn where_json_key_exists(mut self, column: &str, key: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonKeyExists,
            value: ConditionValue::Single(serde_json::Value::String(key.to_string())),
        });
        self
    }

    /// Add a JSON key does not exist condition.
    pub fn where_json_key_not_exists(mut self, column: &str, key: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonKeyNotExists,
            value: ConditionValue::Single(serde_json::Value::String(key.to_string())),
        });
        self
    }

    /// Add a JSON path exists condition.
    pub fn where_json_path_exists(mut self, column: &str, path: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonPathExists,
            value: ConditionValue::Single(serde_json::Value::String(path.to_string())),
        });
        self
    }

    /// Add a JSON path does not exist condition.
    pub fn where_json_path_not_exists(mut self, column: &str, path: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonPathNotExists,
            value: ConditionValue::Single(serde_json::Value::String(path.to_string())),
        });
        self
    }

    /// Add an array contains condition (column @> value).
    pub fn where_array_contains<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContains,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contained by condition (column <@ value).
    pub fn where_array_contained_by<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainedBy,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array overlaps condition (column && value).
    pub fn where_array_overlaps<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayOverlaps,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contains any element condition.
    pub fn where_array_contains_any<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainsAny,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contains all elements condition.
    pub fn where_array_contains_all<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainsAll,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }
}
