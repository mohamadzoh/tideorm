use super::*;
use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{Condition, Expr, ExprTrait, Value};
use crate::model::Model;
use crate::soft_delete::{SoftDeleteScope, query_scope_for};
use sea_orm::sea_query::{
    Alias, MysqlQueryBuilder, PostgresQueryBuilder, Query, SimpleExpr, SqliteQueryBuilder,
    extension::postgres::PgBinOper,
};

#[derive(Clone, Copy)]
enum ComparisonOperator {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Clone, Copy)]
enum ListOperator {
    In,
    NotIn,
    EqAny,
    NeAll,
}

#[derive(Clone, Copy)]
enum JsonValueOperator {
    Contains,
    ContainedBy,
}

#[derive(Clone, Copy)]
enum JsonStringOperator {
    KeyPresent,
    KeyAbsent,
    PathPresent,
    PathAbsent,
}

#[derive(Clone, Copy)]
enum ArrayOperator {
    Contains,
    ContainedBy,
    Overlaps,
}

enum ConditionSpec<'a> {
    Raw {
        column: &'a str,
        raw_sql: &'a str,
    },
    Compare {
        operator: ComparisonOperator,
        value: &'a serde_json::Value,
    },
    Pattern {
        negated: bool,
        escaped: bool,
        value: &'a serde_json::Value,
    },
    List {
        operator: ListOperator,
        values: &'a [serde_json::Value],
    },
    NullCheck {
        negated: bool,
    },
    Between {
        low: &'a serde_json::Value,
        high: &'a serde_json::Value,
    },
    JsonValue {
        operator: JsonValueOperator,
        value: &'a serde_json::Value,
    },
    JsonString {
        operator: JsonStringOperator,
        value: &'a str,
    },
    Array {
        operator: ArrayOperator,
        values: &'a [serde_json::Value],
    },
    Subquery {
        negated: bool,
        query_sql: &'a str,
    },
}

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    fn operator_label(operator: &Operator) -> &'static str {
        match operator {
            Operator::Eq => "=",
            Operator::NotEq => "!=",
            Operator::Gt => ">",
            Operator::Gte => ">=",
            Operator::Lt => "<",
            Operator::Lte => "<=",
            Operator::Like => "LIKE",
            Operator::LikeEscaped => "LIKE",
            Operator::NotLike => "NOT LIKE",
            Operator::In => "IN",
            Operator::NotIn => "NOT IN",
            Operator::IsNull => "IS NULL",
            Operator::IsNotNull => "IS NOT NULL",
            Operator::Between => "BETWEEN",
            Operator::JsonContains => "JSON_CONTAINS",
            Operator::JsonContainedBy => "JSON_CONTAINED_BY",
            Operator::JsonKeyExists => "JSON_KEY_EXISTS",
            Operator::JsonKeyNotExists => "JSON_KEY_NOT_EXISTS",
            Operator::JsonPathExists => "JSON_PATH_EXISTS",
            Operator::JsonPathNotExists => "JSON_PATH_NOT_EXISTS",
            Operator::ArrayContains => "ARRAY_CONTAINS",
            Operator::ArrayContainedBy => "ARRAY_CONTAINED_BY",
            Operator::ArrayOverlaps => "ARRAY_OVERLAPS",
            Operator::ArrayContainsAny => "ARRAY_CONTAINS_ANY",
            Operator::ArrayContainsAll => "ARRAY_CONTAINS_ALL",
            Operator::SubqueryIn => "IN SUBQUERY",
            Operator::SubqueryNotIn => "NOT IN SUBQUERY",
            Operator::Raw => "RAW",
            Operator::EqAny => "= ANY",
            Operator::NeAll => "<> ALL",
        }
    }

    fn describe_condition_value(value: &ConditionValue) -> String {
        match value {
            ConditionValue::Single(value) => value.to_string(),
            ConditionValue::List(values) => format!("{:?}", values),
            ConditionValue::Range(low, high) => format!("{}..{}", low, high),
            ConditionValue::None => "NULL".to_string(),
            ConditionValue::Subquery(query_sql) => query_sql.clone(),
            ConditionValue::RawExpr(raw_sql) => raw_sql.clone(),
        }
    }

    fn describe_condition(condition: &WhereCondition) -> String {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => raw_sql.clone(),
            (Operator::IsNull | Operator::IsNotNull, ConditionValue::None) => {
                format!(
                    "{} {}",
                    condition.column,
                    Self::operator_label(&condition.operator)
                )
            }
            _ => format!(
                "{} {} {}",
                condition.column,
                Self::operator_label(&condition.operator),
                Self::describe_condition_value(&condition.value)
            ),
        }
    }

    fn describe_or_group(group: &OrGroup) -> String {
        let mut parts: Vec<String> = group
            .conditions
            .iter()
            .map(Self::describe_condition)
            .collect();
        parts.extend(group.nested_groups.iter().map(Self::describe_or_group));

        if parts.is_empty() {
            String::new()
        } else if parts.len() == 1 {
            parts[0].clone()
        } else {
            format!(
                "({})",
                parts.join(&format!(" {} ", group.combine_with.as_sql()))
            )
        }
    }

    fn error_context_conditions(&self) -> Vec<String> {
        let mut conditions: Vec<String> = self
            .conditions
            .iter()
            .map(Self::describe_condition)
            .collect();
        conditions.extend(
            self.or_groups
                .iter()
                .map(Self::describe_or_group)
                .filter(|group| !group.is_empty()),
        );
        conditions.extend(
            self.having_conditions
                .iter()
                .map(|having| format!("HAVING {}", having)),
        );
        conditions
    }

    fn error_context_operator_chain(&self) -> Option<String> {
        let mut parts = Vec::new();

        if !self.conditions.is_empty() {
            parts.push(
                self.conditions
                    .iter()
                    .map(Self::describe_condition)
                    .collect::<Vec<_>>()
                    .join(" AND "),
            );
        }

        parts.extend(
            self.or_groups
                .iter()
                .map(Self::describe_or_group)
                .filter(|group| !group.is_empty()),
        );

        if !self.having_conditions.is_empty() {
            parts.push(format!("HAVING {}", self.having_conditions.join(" AND ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" AND "))
        }
    }

    pub(super) fn build_query_error_context(
        &self,
        query: Option<String>,
    ) -> crate::error::ErrorContext {
        let mut context = crate::error::ErrorContext::new()
            .table(M::table_name())
            .conditions(self.error_context_conditions());

        if let Some(operator_chain) = self.error_context_operator_chain() {
            context = context.operator_chain(operator_chain);
        }

        if let Some(query) = query {
            context = context.query(query);
        }

        context
    }

    fn json_to_sea_value(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::Null => Value::String(None),
            serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
            serde_json::Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Value::BigInt(Some(integer))
                } else if let Some(float) = number.as_f64() {
                    Value::Double(Some(float))
                } else {
                    Value::String(Some(number.to_string()))
                }
            }
            serde_json::Value::String(text) => Value::String(Some(text.clone())),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::String(Some(value.to_string()))
            }
        }
    }

    pub(super) fn build_sea_condition(&self) -> Condition {
        self.build_sea_condition_for_db(self.db_type_for_sql())
    }

    fn build_sea_condition_for_db(&self, db_type: DatabaseType) -> Condition {
        let mut condition = Condition::all();

        for filter in &self.conditions {
            if let Some(expression) = self.build_condition_expression(filter, db_type) {
                condition = condition.add(expression);
            }
        }

        for group in &self.or_groups {
            if !group.is_empty() {
                condition = condition.add(self.build_or_group_condition(group, db_type));
            }
        }

        if let Some(soft_delete_expression) = self.build_soft_delete_expression(db_type) {
            condition = condition.add(soft_delete_expression);
        }

        condition
    }

    pub(super) fn db_type_for_sql(&self) -> DatabaseType {
        self.database
            .as_ref()
            .map(|db| db.backend())
            .or_else(|| crate::database::try_db().map(|db| db.backend()))
            .unwrap_or(DatabaseType::Postgres)
    }

    fn current_timestamp_sql() -> &'static str {
        "CURRENT_TIMESTAMP"
    }

    fn sea_value_list(values: &[serde_json::Value]) -> Vec<Value> {
        values.iter().map(Self::json_to_sea_value).collect()
    }

    fn json_text_value(text: String) -> Value {
        Value::String(Some(text))
    }

    fn json_array_parameter(values: &[serde_json::Value]) -> Value {
        Self::json_text_value(
            serde_json::to_string(values)
                .expect("serializing array predicate values should not fail"),
        )
    }

    fn json_scalar_parameter(value: &serde_json::Value) -> Value {
        Self::json_text_value(
            serde_json::to_string(value)
                .expect("serializing scalar predicate value should not fail"),
        )
    }

    fn json_parameter(value: &serde_json::Value) -> Value {
        Value::Json(Some(Box::new(value.clone())))
    }

    fn sqlite_json_compare_value(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::String(text) => Value::String(Some(text.clone())),
            serde_json::Value::Null => Value::String(Some("null".to_string())),
            serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
            serde_json::Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Value::BigInt(Some(integer))
                } else if let Some(float) = number.as_f64() {
                    Value::Double(Some(float))
                } else {
                    Value::String(Some(number.to_string()))
                }
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::String(Some(value.to_string()))
            }
        }
    }

    #[cfg(feature = "postgres")]
    fn postgres_array_value(values: &[serde_json::Value]) -> Value {
        if values.iter().all(|value| value.is_string()) {
            return values
                .iter()
                .map(|value| value.as_str().expect("checked string value").to_string())
                .collect::<Vec<_>>()
                .into();
        }

        if values.iter().all(|value| value.is_boolean()) {
            return values
                .iter()
                .map(|value| value.as_bool().expect("checked boolean value"))
                .collect::<Vec<_>>()
                .into();
        }

        if values.iter().all(|value| value.as_i64().is_some()) {
            return values
                .iter()
                .map(|value| value.as_i64().expect("checked integer value"))
                .collect::<Vec<_>>()
                .into();
        }

        if values.iter().all(|value| value.is_number()) {
            return values
                .iter()
                .map(|value| value.as_f64().expect("checked numeric value"))
                .collect::<Vec<_>>()
                .into();
        }

        values.to_vec().into()
    }

    fn placeholder_list(count: usize) -> String {
        std::iter::repeat_n("?", count)
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn sea_column_expr(&self, db_type: DatabaseType, column: &str) -> SimpleExpr {
        if column.contains('(')
            || column.contains('*')
            || column.contains(' ')
            || column.contains('"')
            || column.contains('`')
        {
            return Expr::cust(self.format_column_for_db(db_type, column));
        }

        if let Some((table, field)) = column.split_once('.') {
            if db_sql::validate_identifier("table", table).is_ok()
                && db_sql::validate_identifier("column", field).is_ok()
            {
                return Expr::col((Alias::new(table), Alias::new(field)));
            }
        } else if db_sql::validate_identifier("column", column).is_ok() {
            return Expr::col(Alias::new(column));
        }

        Expr::cust(self.format_column_for_db(db_type, column))
    }

    fn build_custom_expression(&self, sql: String, values: Vec<Value>) -> SimpleExpr {
        if values.is_empty() {
            Expr::cust(sql)
        } else {
            Expr::cust_with_values(sql, values)
        }
    }

    fn condition_spec<'a>(condition: &'a WhereCondition) -> Option<ConditionSpec<'a>> {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => Some(ConditionSpec::Raw {
                column: &condition.column,
                raw_sql,
            }),
            (Operator::Eq, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Eq,
                value,
            }),
            (Operator::NotEq, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::NotEq,
                value,
            }),
            (Operator::Gt, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Gt,
                value,
            }),
            (Operator::Gte, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Gte,
                value,
            }),
            (Operator::Lt, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Lt,
                value,
            }),
            (Operator::Lte, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Lte,
                value,
            }),
            (Operator::Like, ConditionValue::Single(value)) => Some(ConditionSpec::Pattern {
                negated: false,
                escaped: false,
                value,
            }),
            (Operator::LikeEscaped, ConditionValue::Single(value)) => {
                Some(ConditionSpec::Pattern {
                    negated: false,
                    escaped: true,
                    value,
                })
            }
            (Operator::NotLike, ConditionValue::Single(value)) => Some(ConditionSpec::Pattern {
                negated: true,
                escaped: false,
                value,
            }),
            (Operator::In, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::In,
                values,
            }),
            (Operator::NotIn, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::NotIn,
                values,
            }),
            (Operator::EqAny, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::EqAny,
                values,
            }),
            (Operator::NeAll, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::NeAll,
                values,
            }),
            (Operator::IsNull, ConditionValue::None) => {
                Some(ConditionSpec::NullCheck { negated: false })
            }
            (Operator::IsNotNull, ConditionValue::None) => {
                Some(ConditionSpec::NullCheck { negated: true })
            }
            (Operator::Between, ConditionValue::Range(low, high)) => {
                Some(ConditionSpec::Between { low, high })
            }
            (Operator::JsonContains, ConditionValue::Single(value)) => {
                Some(ConditionSpec::JsonValue {
                    operator: JsonValueOperator::Contains,
                    value,
                })
            }
            (Operator::JsonContainedBy, ConditionValue::Single(value)) => {
                Some(ConditionSpec::JsonValue {
                    operator: JsonValueOperator::ContainedBy,
                    value,
                })
            }
            (Operator::JsonKeyExists, ConditionValue::Single(serde_json::Value::String(value))) => {
                Some(ConditionSpec::JsonString {
                    operator: JsonStringOperator::KeyPresent,
                    value,
                })
            }
            (
                Operator::JsonKeyNotExists,
                ConditionValue::Single(serde_json::Value::String(value)),
            ) => Some(ConditionSpec::JsonString {
                operator: JsonStringOperator::KeyAbsent,
                value,
            }),
            (
                Operator::JsonPathExists,
                ConditionValue::Single(serde_json::Value::String(value)),
            ) => Some(ConditionSpec::JsonString {
                operator: JsonStringOperator::PathPresent,
                value,
            }),
            (
                Operator::JsonPathNotExists,
                ConditionValue::Single(serde_json::Value::String(value)),
            ) => Some(ConditionSpec::JsonString {
                operator: JsonStringOperator::PathAbsent,
                value,
            }),
            (Operator::ArrayContains, ConditionValue::List(values))
            | (Operator::ArrayContainsAll, ConditionValue::List(values)) => {
                Some(ConditionSpec::Array {
                    operator: ArrayOperator::Contains,
                    values,
                })
            }
            (Operator::ArrayContainedBy, ConditionValue::List(values)) => {
                Some(ConditionSpec::Array {
                    operator: ArrayOperator::ContainedBy,
                    values,
                })
            }
            (Operator::ArrayOverlaps, ConditionValue::List(values))
            | (Operator::ArrayContainsAny, ConditionValue::List(values)) => {
                Some(ConditionSpec::Array {
                    operator: ArrayOperator::Overlaps,
                    values,
                })
            }
            (Operator::SubqueryIn, ConditionValue::Subquery(query_sql)) => {
                Some(ConditionSpec::Subquery {
                    negated: false,
                    query_sql,
                })
            }
            (Operator::SubqueryNotIn, ConditionValue::Subquery(query_sql)) => {
                Some(ConditionSpec::Subquery {
                    negated: true,
                    query_sql,
                })
            }
            _ => None,
        }
    }

    fn preview_values(&self, values: &[serde_json::Value]) -> Vec<String> {
        values
            .iter()
            .map(|value| self.format_preview_value(value))
            .collect()
    }

    fn pattern_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(text) => text.clone(),
            _ => value.to_string(),
        }
    }

    fn comparison_sql(operator: ComparisonOperator) -> &'static str {
        match operator {
            ComparisonOperator::Eq => "=",
            ComparisonOperator::NotEq => "!=",
            ComparisonOperator::Gt => ">",
            ComparisonOperator::Gte => ">=",
            ComparisonOperator::Lt => "<",
            ComparisonOperator::Lte => "<=",
        }
    }

    fn build_raw_condition_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        raw_sql: &str,
    ) -> SimpleExpr {
        if column.is_empty() {
            Expr::cust(raw_sql.to_string())
        } else {
            let column = self.format_column_for_db(db_type, column);
            Expr::cust(format!("{} {}", column, raw_sql))
        }
    }

    fn build_raw_condition_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        raw_sql: &str,
    ) -> String {
        if column.is_empty() {
            raw_sql.to_string()
        } else {
            format!("{} {}", self.format_column_for_db(db_type, column), raw_sql)
        }
    }

    fn build_compare_expression(
        &self,
        column_expr: SimpleExpr,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let value = Self::json_to_sea_value(value);
        match operator {
            ComparisonOperator::Eq => column_expr.eq(value),
            ComparisonOperator::NotEq => column_expr.ne(value),
            ComparisonOperator::Gt => column_expr.gt(value),
            ComparisonOperator::Gte => column_expr.gte(value),
            ComparisonOperator::Lt => column_expr.lt(value),
            ComparisonOperator::Lte => column_expr.lte(value),
        }
    }

    fn build_compare_sql(
        &self,
        column: &str,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> String {
        format!(
            "{} {} {}",
            column,
            Self::comparison_sql(operator),
            self.format_preview_value(value)
        )
    }

    fn build_pattern_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        negated: bool,
        escaped: bool,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let pattern = Self::pattern_value(value);
        if escaped {
            let operator = if negated { "NOT LIKE" } else { "LIKE" };
            let placeholder = match db_type {
                DatabaseType::Postgres => "$1",
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => "?",
            };
            let escape_clause = match db_type {
                DatabaseType::Postgres => " ESCAPE '\\'",
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                    " ESCAPE '\\\\'"
                }
            };
            self.build_custom_expression(
                format!(
                    "{} {} {}{}",
                    column_sql, operator, placeholder, escape_clause
                ),
                vec![Value::String(Some(pattern))],
            )
        } else if negated {
            column_expr.not_like(pattern)
        } else {
            column_expr.like(pattern)
        }
    }

    fn build_pattern_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        negated: bool,
        escaped: bool,
        value: &serde_json::Value,
    ) -> String {
        let mut sql = format!(
            "{} {}LIKE {}",
            column,
            if negated { "NOT " } else { "" },
            self.format_preview_value(value)
        );
        if escaped {
            sql.push_str(match db_type {
                DatabaseType::Postgres => " ESCAPE '\\'",
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                    " ESCAPE '\\\\'"
                }
            });
        }
        sql
    }

    fn build_list_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        let sea_values = Self::sea_value_list(values);
        match operator {
            ListOperator::In => column_expr.is_in(sea_values),
            ListOperator::NotIn => column_expr.is_not_in(sea_values),
            ListOperator::EqAny if matches!(db_type, DatabaseType::Postgres) => self
                .build_custom_expression(
                    format!(
                        "{} = ANY(ARRAY[{}])",
                        column_sql,
                        Self::placeholder_list(values.len())
                    ),
                    sea_values,
                ),
            ListOperator::EqAny => column_expr.is_in(sea_values),
            ListOperator::NeAll if matches!(db_type, DatabaseType::Postgres) => self
                .build_custom_expression(
                    format!(
                        "{} <> ALL(ARRAY[{}])",
                        column_sql,
                        Self::placeholder_list(values.len())
                    ),
                    sea_values,
                ),
            ListOperator::NeAll => column_expr.is_not_in(sea_values),
        }
    }

    fn build_list_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> String {
        let rendered = self.preview_values(values);
        match operator {
            ListOperator::In => format!("{} IN ({})", column, rendered.join(", ")),
            ListOperator::NotIn => format!("{} NOT IN ({})", column, rendered.join(", ")),
            ListOperator::EqAny => db_sql::eq_any(db_type, column, &rendered),
            ListOperator::NeAll => db_sql::ne_all(db_type, column, &rendered),
        }
    }

    fn build_null_check_expression(&self, column_expr: SimpleExpr, negated: bool) -> SimpleExpr {
        if negated {
            column_expr.is_not_null()
        } else {
            column_expr.is_null()
        }
    }

    fn build_null_check_sql(&self, column: &str, negated: bool) -> String {
        format!("{} IS {}NULL", column, if negated { "NOT " } else { "" })
    }

    fn build_between_expression(
        &self,
        column_expr: SimpleExpr,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> SimpleExpr {
        column_expr.between(Self::json_to_sea_value(low), Self::json_to_sea_value(high))
    }

    fn build_between_sql(
        &self,
        column: &str,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> String {
        format!(
            "{} BETWEEN {} AND {}",
            column,
            self.format_preview_value(low),
            self.format_preview_value(high)
        )
    }

    fn build_json_value_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: JsonValueOperator,
        value: &serde_json::Value,
    ) -> String {
        match operator {
            JsonValueOperator::Contains => {
                db_sql::json_contains(db_type, column, &value.to_string())
            }
            JsonValueOperator::ContainedBy => {
                db_sql::json_contained_by(db_type, column, &value.to_string())
            }
        }
    }

    fn build_json_value_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        operator: JsonValueOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        match db_type {
            DatabaseType::Postgres => {
                match operator {
                    JsonValueOperator::Contains => column_expr
                        .binary(PgBinOper::Contains, Expr::val(Self::json_parameter(value))),
                    JsonValueOperator::ContainedBy => column_expr
                        .binary(PgBinOper::Contained, Expr::val(Self::json_parameter(value))),
                }
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => match operator {
                JsonValueOperator::Contains => self.build_custom_expression(
                    format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
                    vec![Self::json_scalar_parameter(value)],
                ),
                JsonValueOperator::ContainedBy => self.build_custom_expression(
                    format!("JSON_CONTAINS(CAST(? AS JSON), {})", column_sql),
                    vec![Self::json_scalar_parameter(value)],
                ),
            },
            DatabaseType::SQLite => match operator {
                JsonValueOperator::Contains => self.build_custom_expression(
                    format!(
                        "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                        column_sql
                    ),
                    vec![Self::sqlite_json_compare_value(value)],
                ),
                JsonValueOperator::ContainedBy => self.build_custom_expression(
                    format!(
                        "json_type({}) IS NOT NULL AND ? LIKE '%' || {} || '%'",
                        column_sql, column_sql
                    ),
                    vec![Self::json_scalar_parameter(value)],
                ),
            },
        }
    }

    fn build_json_string_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: JsonStringOperator,
        value: &str,
    ) -> String {
        match operator {
            JsonStringOperator::KeyPresent => db_sql::json_key_exists(db_type, column, value),
            JsonStringOperator::KeyAbsent => db_sql::json_key_not_exists(db_type, column, value),
            JsonStringOperator::PathPresent => db_sql::json_path_exists(db_type, column, value),
            JsonStringOperator::PathAbsent => db_sql::json_path_not_exists(db_type, column, value),
        }
    }

    fn build_json_string_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: JsonStringOperator,
        value: &str,
    ) -> SimpleExpr {
        match operator {
            JsonStringOperator::KeyPresent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("{} ? $1", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => self.build_custom_expression(
                    format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
                DatabaseType::SQLite => self.build_custom_expression(
                    format!("json_extract({}, ?) IS NOT NULL", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
            },
            JsonStringOperator::KeyAbsent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("NOT ({} ? $1)", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => self.build_custom_expression(
                    format!("NOT JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
                DatabaseType::SQLite => self.build_custom_expression(
                    format!("json_extract({}, ?) IS NULL", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
            },
            JsonStringOperator::PathPresent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("{} @? ($1::jsonpath)", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(true));
                    };
                    self.build_custom_expression(
                        format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
                DatabaseType::SQLite => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(true));
                    };
                    self.build_custom_expression(
                        format!("json_extract({}, ?) IS NOT NULL", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
            },
            JsonStringOperator::PathAbsent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("NOT ({} @? ($1::jsonpath))", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(false));
                    };
                    self.build_custom_expression(
                        format!("NOT JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
                DatabaseType::SQLite => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(false));
                    };
                    self.build_custom_expression(
                        format!("json_extract({}, ?) IS NULL", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
            },
        }
    }

    fn build_array_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: ArrayOperator,
        values: &[serde_json::Value],
    ) -> String {
        let rendered = self.render_array_values(values);
        match operator {
            ArrayOperator::Contains => db_sql::array_contains(db_type, column, &rendered),
            ArrayOperator::ContainedBy => db_sql::array_contained_by(db_type, column, &rendered),
            ArrayOperator::Overlaps => db_sql::array_overlaps(db_type, column, &rendered),
        }
    }

    fn build_array_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        operator: ArrayOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        match db_type {
            DatabaseType::Postgres => {
                if values.is_empty() {
                    return Expr::cust(self.build_array_sql(db_type, column_sql, operator, values));
                }

                #[cfg(feature = "postgres")]
                {
                    let operator = match operator {
                        ArrayOperator::Contains => PgBinOper::Contains,
                        ArrayOperator::ContainedBy => PgBinOper::Contained,
                        ArrayOperator::Overlaps => PgBinOper::Overlap,
                    };

                    column_expr.binary(operator, Expr::val(Self::postgres_array_value(values)))
                }

                #[cfg(not(feature = "postgres"))]
                {
                    let _ = column_expr;
                    Expr::cust(self.build_array_sql(db_type, column_sql, operator, values))
                }
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => match operator {
                ArrayOperator::Contains => self.build_custom_expression(
                    format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
                    vec![Self::json_array_parameter(values)],
                ),
                ArrayOperator::ContainedBy => self.build_custom_expression(
                    format!("JSON_CONTAINS(CAST(? AS JSON), {})", column_sql),
                    vec![Self::json_array_parameter(values)],
                ),
                ArrayOperator::Overlaps => {
                    if values.is_empty() {
                        Expr::cust("0 = 1".to_string())
                    } else {
                        let sql = std::iter::repeat_n(
                            format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
                            values.len(),
                        )
                        .collect::<Vec<_>>()
                        .join(" OR ");
                        let params = values.iter().map(Self::json_scalar_parameter).collect();
                        self.build_custom_expression(format!("({})", sql), params)
                    }
                }
            },
            DatabaseType::SQLite => match operator {
                ArrayOperator::Contains => {
                    if values.is_empty() {
                        Expr::cust("1 = 1".to_string())
                    } else {
                        let sql = std::iter::repeat_n(
                            format!(
                                "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                                column_sql
                            ),
                            values.len(),
                        )
                        .collect::<Vec<_>>()
                        .join(" AND ");
                        self.build_custom_expression(
                            format!("({})", sql),
                            Self::sea_value_list(values),
                        )
                    }
                }
                ArrayOperator::ContainedBy => {
                    if values.is_empty() {
                        Expr::cust(format!(
                            "NOT EXISTS (SELECT 1 FROM json_each({}))",
                            column_sql
                        ))
                    } else {
                        self.build_custom_expression(
                            format!(
                                "NOT EXISTS (SELECT 1 FROM json_each({}) WHERE value NOT IN ({}))",
                                column_sql,
                                Self::placeholder_list(values.len())
                            ),
                            Self::sea_value_list(values),
                        )
                    }
                }
                ArrayOperator::Overlaps => {
                    if values.is_empty() {
                        Expr::cust("0 = 1".to_string())
                    } else {
                        let sql = std::iter::repeat_n(
                            format!(
                                "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                                column_sql
                            ),
                            values.len(),
                        )
                        .collect::<Vec<_>>()
                        .join(" OR ");
                        self.build_custom_expression(
                            format!("({})", sql),
                            Self::sea_value_list(values),
                        )
                    }
                }
            },
        }
    }

    fn build_subquery_expression(
        &self,
        column_sql: &str,
        negated: bool,
        query_sql: &str,
    ) -> SimpleExpr {
        Expr::cust(format!(
            "{} {}IN ({})",
            column_sql,
            if negated { "NOT " } else { "" },
            query_sql
        ))
    }

    fn build_subquery_sql(&self, column: &str, negated: bool, query_sql: &str) -> String {
        format!(
            "{} {}IN ({})",
            column,
            if negated { "NOT " } else { "" },
            query_sql
        )
    }

    fn build_condition_expression(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<SimpleExpr> {
        let spec = Self::condition_spec(condition)?;

        if let ConditionSpec::Raw { column, raw_sql } = spec {
            return Some(self.build_raw_condition_expression(db_type, column, raw_sql));
        }

        let column_expr = self.sea_column_expr(db_type, &condition.column);
        let column_sql = self.format_column_for_db(db_type, &condition.column);

        match spec {
            ConditionSpec::Raw { .. } => None,
            ConditionSpec::Compare { operator, value } => {
                Some(self.build_compare_expression(column_expr, operator, value))
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => Some(self.build_pattern_expression(
                db_type,
                column_expr,
                &column_sql,
                negated,
                escaped,
                value,
            )),
            ConditionSpec::List { operator, values } => Some(self.build_list_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                values,
            )),
            ConditionSpec::NullCheck { negated } => {
                Some(self.build_null_check_expression(column_expr, negated))
            }
            ConditionSpec::Between { low, high } => {
                Some(self.build_between_expression(column_expr, low, high))
            }
            ConditionSpec::JsonValue { operator, value } => Some(self.build_json_value_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                value,
            )),
            ConditionSpec::JsonString { operator, value } => {
                Some(self.build_json_string_expression(db_type, &column_sql, operator, value))
            }
            ConditionSpec::Array { operator, values } => Some(self.build_array_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                values,
            )),
            ConditionSpec::Subquery { negated, query_sql } => {
                Some(self.build_subquery_expression(&column_sql, negated, query_sql))
            }
        }
    }

    fn build_or_group_condition(&self, group: &OrGroup, db_type: DatabaseType) -> Condition {
        let mut condition = match group.combine_with {
            LogicalOp::And => Condition::all(),
            LogicalOp::Or => Condition::any(),
        };

        for filter in &group.conditions {
            if let Some(expression) = self.build_condition_expression(filter, db_type) {
                condition = condition.add(expression);
            }
        }

        for nested_group in &group.nested_groups {
            if !nested_group.is_empty() {
                condition = condition.add(self.build_or_group_condition(nested_group, db_type));
            }
        }

        condition
    }

    fn build_soft_delete_expression(&self, db_type: DatabaseType) -> Option<SimpleExpr> {
        match query_scope_for::<M>(self.include_trashed, self.only_trashed) {
            SoftDeleteScope::Disabled | SoftDeleteScope::WithTrashed => None,
            SoftDeleteScope::ActiveOnly => Some(
                self.sea_column_expr(db_type, M::deleted_at_column())
                    .is_null(),
            ),
            SoftDeleteScope::OnlyTrashed => Some(
                self.sea_column_expr(db_type, M::deleted_at_column())
                    .is_not_null(),
            ),
        }
    }

    pub(crate) fn build_where_clause_with_condition_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let has_filters = !self.conditions.is_empty()
            || !self.or_groups.is_empty()
            || self.build_soft_delete_expression(db_type).is_some();
        if !has_filters {
            return (String::new(), Vec::new());
        }

        let mut query = Query::select();
        query.expr(Expr::cust("1"));
        query.cond_where(self.build_sea_condition_for_db(db_type));

        let (sql, values) = match db_type {
            DatabaseType::Postgres => query.build(PostgresQueryBuilder),
            DatabaseType::MySQL | DatabaseType::MariaDB => query.build(MysqlQueryBuilder),
            DatabaseType::SQLite => query.build(SqliteQueryBuilder),
        };

        match sql.split_once(" WHERE ") {
            Some((_, where_sql)) => (where_sql.to_string(), values.into_iter().collect()),
            None => (String::new(), Vec::new()),
        }
    }

    fn format_preview_value(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(boolean) => boolean.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::String(text) => format!("'{}'", text.replace("'", "''")),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                format!("'{}'", value.to_string().replace("'", "''"))
            }
        }
    }

    fn format_column_for_db(&self, db_type: DatabaseType, column: &str) -> String {
        let trimmed = column.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        match parts.as_slice() {
            [identifier] => db_sql::format_column(db_type, identifier),
            [identifier, direction]
                if direction.eq_ignore_ascii_case("asc")
                    || direction.eq_ignore_ascii_case("desc") =>
            {
                db_sql::format_identifier_reference(db_type, identifier)
                    .map(|identifier| format!("{} {}", identifier, direction.to_ascii_uppercase()))
                    .unwrap_or_else(|| trimmed.to_string())
            }
            [identifier, as_keyword, alias] if as_keyword.eq_ignore_ascii_case("as") => {
                match (
                    db_sql::format_identifier_reference(db_type, identifier),
                    db_sql::format_identifier_reference(db_type, alias),
                ) {
                    (Some(identifier), Some(alias)) => format!("{} AS {}", identifier, alias),
                    _ => trimmed.to_string(),
                }
            }
            _ => trimmed.to_string(),
        }
    }

    fn format_select_column_for_db(
        &self,
        db_type: DatabaseType,
        table: &str,
        column: &str,
    ) -> String {
        let trimmed = column.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        match parts.as_slice() {
            [identifier]
                if !identifier.contains('(')
                    && !identifier.contains('*')
                    && db_sql::format_identifier_reference(db_type, identifier).is_some() =>
            {
                if identifier.contains('.') {
                    self.format_column_for_db(db_type, identifier)
                } else {
                    format!(
                        "{}.{}",
                        db_sql::quote_ident(db_type, table),
                        db_sql::quote_ident(db_type, identifier)
                    )
                }
            }
            [identifier, as_keyword, alias]
                if as_keyword.eq_ignore_ascii_case("as")
                    && !identifier.contains('(')
                    && !identifier.contains('*')
                    && db_sql::format_identifier_reference(db_type, identifier).is_some()
                    && db_sql::format_identifier_reference(db_type, alias).is_some() =>
            {
                let identifier = if identifier.contains('.') {
                    self.format_column_for_db(db_type, identifier)
                } else {
                    format!(
                        "{}.{}",
                        db_sql::quote_ident(db_type, table),
                        db_sql::quote_ident(db_type, identifier)
                    )
                };

                format!("{} AS {}", identifier, db_sql::quote_ident(db_type, alias))
            }
            _ => trimmed.to_string(),
        }
    }

    fn build_select_clause_sql(&self, db_type: DatabaseType) -> String {
        let table = M::table_name();

        if !self.raw_select_expressions.is_empty() {
            let mut expressions = self.raw_select_expressions.clone();
            for window_function in &self.window_functions {
                expressions.push(window_function.to_sql_for_db(db_type));
            }
            return format!("SELECT {} ", expressions.join(", "));
        }

        if let Some(columns) = &self.select_columns {
            let mut rendered_columns: Vec<String> = columns
                .iter()
                .map(|column| self.format_select_column_for_db(db_type, table, column))
                .collect();

            for window_function in &self.window_functions {
                rendered_columns.push(window_function.to_sql_for_db(db_type));
            }

            return format!("SELECT {} ", rendered_columns.join(", "));
        }

        let mut select_parts = vec![format!("{}.*", db_sql::quote_ident(db_type, table))];
        for window_function in &self.window_functions {
            select_parts.push(window_function.to_sql_for_db(db_type));
        }
        format!("SELECT {} ", select_parts.join(", "))
    }

    fn append_from_and_join_sql(&self, sql: &mut String, db_type: DatabaseType) {
        sql.push_str(&format!(
            "FROM {} ",
            db_sql::quote_ident(db_type, M::table_name())
        ));

        for join in &self.joins {
            let join_table = if let Some(alias) = &join.alias {
                format!(
                    "{} AS {}",
                    db_sql::quote_ident(db_type, &join.table),
                    db_sql::quote_ident(db_type, alias)
                )
            } else {
                db_sql::quote_ident(db_type, &join.table)
            };

            sql.push_str(&format!(
                "{} {} ON {} = {} ",
                join.join_type.as_sql(),
                join_table,
                self.format_column_for_db(db_type, &join.left_column),
                self.format_column_for_db(db_type, &join.right_column)
            ));
        }
    }

    fn append_group_by_and_having_sql(&self, sql: &mut String, db_type: DatabaseType) {
        if !self.group_by.is_empty() {
            let columns: Vec<String> = self
                .group_by
                .iter()
                .map(|column| self.format_column_for_db(db_type, column))
                .collect();
            sql.push_str(&format!("GROUP BY {} ", columns.join(", ")));
        }

        if !self.having_conditions.is_empty() {
            sql.push_str(&format!("HAVING {} ", self.having_conditions.join(" AND ")));
        }
    }

    fn build_condition_sql_for_db(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<String> {
        let spec = Self::condition_spec(condition)?;

        if let ConditionSpec::Raw { column, raw_sql } = spec {
            return Some(self.build_raw_condition_sql(db_type, column, raw_sql));
        }

        let column = self.format_column_for_db(db_type, &condition.column);

        match spec {
            ConditionSpec::Raw { .. } => None,
            ConditionSpec::Compare { operator, value } => {
                Some(self.build_compare_sql(&column, operator, value))
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => Some(self.build_pattern_sql(db_type, &column, negated, escaped, value)),
            ConditionSpec::List { operator, values } => {
                Some(self.build_list_sql(db_type, &column, operator, values))
            }
            ConditionSpec::NullCheck { negated } => {
                Some(self.build_null_check_sql(&column, negated))
            }
            ConditionSpec::Between { low, high } => {
                Some(self.build_between_sql(&column, low, high))
            }
            ConditionSpec::JsonValue { operator, value } => {
                Some(self.build_json_value_sql(db_type, &condition.column, operator, value))
            }
            ConditionSpec::JsonString { operator, value } => {
                Some(self.build_json_string_sql(db_type, &condition.column, operator, value))
            }
            ConditionSpec::Array { operator, values } => {
                Some(self.build_array_sql(db_type, &condition.column, operator, values))
            }
            ConditionSpec::Subquery { negated, query_sql } => {
                Some(self.build_subquery_sql(&column, negated, query_sql))
            }
        }
    }

    fn render_array_values(&self, values: &[serde_json::Value]) -> Vec<String> {
        values
            .iter()
            .map(|value| match value {
                serde_json::Value::String(text) => format!("'{}'", text.replace("'", "''")),
                serde_json::Value::Number(number) => number.to_string(),
                serde_json::Value::Bool(boolean) => boolean.to_string(),
                serde_json::Value::Null => "NULL".to_string(),
                _ => format!("'{}'", value.to_string().replace("'", "''")),
            })
            .collect()
    }

    fn build_or_group_sql_for_db(&self, group: &OrGroup, db_type: DatabaseType) -> String {
        let mut parts = Vec::new();

        for condition in &group.conditions {
            if let Some(expression) = self.build_condition_sql_for_db(condition, db_type) {
                parts.push(expression);
            }
        }

        for nested_group in &group.nested_groups {
            let nested_sql = self.build_or_group_sql_for_db(nested_group, db_type);
            if !nested_sql.is_empty() {
                parts.push(format!("({})", nested_sql));
            }
        }

        parts.join(&format!(" {} ", group.combine_with.as_sql()))
    }

    pub(crate) fn build_where_sql_for_db(&self, db_type: DatabaseType) -> String {
        let mut clauses = Vec::new();

        for condition in &self.conditions {
            if let Some(expression) = self.build_condition_sql_for_db(condition, db_type) {
                clauses.push(expression);
            }
        }

        for group in &self.or_groups {
            let group_sql = self.build_or_group_sql_for_db(group, db_type);
            if !group_sql.is_empty() {
                clauses.push(format!("({})", group_sql));
            }
        }

        if M::soft_delete_enabled() {
            let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
            if self.only_trashed {
                clauses.push(format!("{} IS NOT NULL", deleted_at));
            } else if !self.include_trashed {
                clauses.push(format!("{} IS NULL", deleted_at));
            }
        }

        clauses.join(" AND ")
    }

    pub(super) fn build_base_select_sql(&self) -> String {
        let db_type = self.db_type_for_sql();
        let mut sql = String::new();

        sql.push_str(&self.build_select_clause_sql(db_type));
        self.append_from_and_join_sql(&mut sql, db_type);

        let where_sql = self.build_where_sql_for_db(db_type);
        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {} ", where_sql));
        }

        self.append_group_by_and_having_sql(&mut sql, db_type);
        sql.trim().to_string()
    }

    fn build_base_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut sql = String::new();

        sql.push_str(&self.build_select_clause_sql(db_type));
        self.append_from_and_join_sql(&mut sql, db_type);

        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {} ", where_sql));
        }

        self.append_group_by_and_having_sql(&mut sql, db_type);
        (sql.trim().to_string(), params)
    }

    pub(super) fn build_select_sql(&self) -> String {
        let db_type = self.db_type_for_sql();
        let mut sql = String::new();

        if !self.ctes.is_empty() {
            let recursive = self.ctes.iter().any(|cte| cte.recursive);
            sql.push_str(if recursive {
                "WITH RECURSIVE "
            } else {
                "WITH "
            });
            let cte_parts: Vec<String> = self.ctes.iter().map(CTE::to_sql).collect();
            sql.push_str(&cte_parts.join(", "));
            sql.push(' ');
        }

        sql.push_str(&self.build_base_select_sql());

        for union in &self.unions {
            sql.push_str(&format!(
                " {} {}",
                union.union_type.as_sql(),
                union.query_sql
            ));
        }

        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self
                .order_by
                .iter()
                .map(|(column, direction)| {
                    format!(
                        "{} {}",
                        self.format_column_for_db(db_type, column),
                        direction.as_str()
                    )
                })
                .collect();
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }

        if let Some(limit) = self.limit_value {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        if let Some(offset) = self.offset_value {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        sql.trim().to_string()
    }

    pub(crate) fn build_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut sql = String::new();

        if !self.ctes.is_empty() {
            let recursive = self.ctes.iter().any(|cte| cte.recursive);
            sql.push_str(if recursive {
                "WITH RECURSIVE "
            } else {
                "WITH "
            });
            let cte_parts: Vec<String> = self.ctes.iter().map(CTE::to_sql).collect();
            sql.push_str(&cte_parts.join(", "));
            sql.push(' ');
        }

        let (base_sql, params) = self.build_base_select_sql_with_params_for_db(db_type);
        sql.push_str(&base_sql);

        for union in &self.unions {
            sql.push_str(&format!(
                " {} ({})",
                union.union_type.as_sql(),
                union.query_sql
            ));
        }

        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self
                .order_by
                .iter()
                .map(|(column, direction)| {
                    format!(
                        "{} {}",
                        self.format_column_for_db(db_type, column),
                        direction.as_str()
                    )
                })
                .collect();
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }

        if let Some(limit) = self.limit_value {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        if let Some(offset) = self.offset_value {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        (sql.trim().to_string(), params)
    }

    fn build_select_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_select_sql_with_params_for_db(self.db_type_for_sql())
    }

    fn build_count_sql_with_params_for_db(&self, db_type: DatabaseType) -> (String, Vec<Value>) {
        let mut count_query = self.clone();
        count_query.order_by.clear();
        count_query.limit_value = None;
        count_query.offset_value = None;

        let (inner_sql, params) = count_query.build_select_sql_with_params_for_db(db_type);
        (
            format!(
                "SELECT COUNT(*) AS count FROM ({}) AS {}",
                inner_sql,
                db_sql::quote_ident(db_type, "tideorm_count_subquery")
            ),
            params,
        )
    }

    fn build_count_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_count_sql_with_params_for_db(self.db_type_for_sql())
    }

    fn build_exists_sql_with_params_for_db(&self, db_type: DatabaseType) -> (String, Vec<Value>) {
        let mut exists_query = self.clone();
        exists_query.order_by.clear();
        exists_query.limit_value = None;
        exists_query.offset_value = None;

        if exists_query.unions.is_empty() {
            exists_query.select_columns = None;
            exists_query.raw_select_expressions = vec!["1".to_string()];
            exists_query.window_functions.clear();
        }

        let (inner_sql, params) = exists_query.build_select_sql_with_params_for_db(db_type);
        (
            format!(
                "SELECT 1 FROM ({}) AS {} LIMIT 1",
                inner_sql,
                db_sql::quote_ident(db_type, "tideorm_exists_subquery")
            ),
            params,
        )
    }

    fn build_exists_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_exists_sql_with_params_for_db(self.db_type_for_sql())
    }

    fn log_query(&self, sql: &str) {
        if std::env::var("TIDE_LOG_QUERIES")
            .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
            .unwrap_or(false)
        {
            crate::tide_debug!("Query: {}", sql);
        }

        if crate::logging::QueryLogger::is_enabled() {
            let entry = crate::logging::QueryLogEntry::new(sql).with_table(M::table_name());
            crate::logging::QueryLogger::log(entry);
        }
    }

    pub fn debug(&self) -> crate::logging::QueryDebugInfo {
        use crate::logging::QueryDebugInfo;

        let (parameterized_sql, params) = self.build_select_sql_with_params();
        let preview_sql = self.build_sql_preview();
        let mut info = QueryDebugInfo::new(M::table_name()).with_sql(preview_sql.clone());
        info.params = params
            .into_iter()
            .map(|value| format!("{:?}", value))
            .collect();

        for condition in &self.conditions {
            info.add_condition(Self::describe_condition(condition));
        }

        for (column, direction) in &self.order_by {
            info.add_order_by(format!("{} {}", column, direction.as_str()));
        }

        info.group_by = self.group_by.clone();
        info.limit = self.limit_value;
        info.offset = self.offset_value;

        if !self.raw_select_expressions.is_empty() {
            info.select = self.raw_select_expressions.clone();
        } else if let Some(columns) = &self.select_columns {
            info.select = columns.clone();
        }

        for join in &self.joins {
            info.joins.push(format!(
                "{:?} JOIN {} ON {} = {}",
                join.join_type, join.table, join.left_column, join.right_column
            ));
        }

        if !parameterized_sql.is_empty() {
            info.sql = format!(
                "{}\n-- PARAMETERIZED SQL\n{}",
                preview_sql, parameterized_sql
            );
        }

        info
    }

    pub fn build_sql_preview(&self) -> String {
        format!(
            "-- DEBUG PREVIEW (not executable, values are approximate)\n{}",
            self.build_select_sql()
        )
    }

    pub fn cache(mut self, ttl: std::time::Duration) -> Self {
        self.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }

    pub fn cache_with_key(mut self, key: &str, ttl: std::time::Duration) -> Self {
        self.cache_key = Some(key.to_string());
        self.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }

    pub fn cache_with_options(mut self, options: crate::cache::CacheOptions) -> Self {
        self.cache_options = Some(options);
        self
    }

    pub fn no_cache(mut self) -> Self {
        self.cache_options = None;
        self.cache_key = None;
        self
    }

    fn generate_cache_key(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        if let Some(key) = &self.cache_key {
            return key.clone();
        }

        let mut hasher = DefaultHasher::new();
        M::table_name().hash(&mut hasher);

        for condition in &self.conditions {
            condition.column.hash(&mut hasher);
            format!("{:?}", condition.operator).hash(&mut hasher);
            format!("{:?}", condition.value).hash(&mut hasher);
        }

        for group in &self.or_groups {
            format!("{:?}", group).hash(&mut hasher);
        }

        for (column, direction) in &self.order_by {
            column.hash(&mut hasher);
            direction.as_str().hash(&mut hasher);
        }

        self.limit_value.hash(&mut hasher);
        self.offset_value.hash(&mut hasher);
        self.include_trashed.hash(&mut hasher);
        self.only_trashed.hash(&mut hasher);
        self.select_columns.hash(&mut hasher);

        for raw_select in &self.raw_select_expressions {
            raw_select.hash(&mut hasher);
        }

        for join in &self.joins {
            join.table.hash(&mut hasher);
            join.alias.hash(&mut hasher);
            join.left_column.hash(&mut hasher);
            join.right_column.hash(&mut hasher);
        }

        for column in &self.group_by {
            column.hash(&mut hasher);
        }

        for having in &self.having_conditions {
            having.hash(&mut hasher);
        }

        for union in &self.unions {
            union.query_sql.hash(&mut hasher);
            union.union_type.as_sql().hash(&mut hasher);
        }

        for cte in &self.ctes {
            cte.name.hash(&mut hasher);
            cte.query_sql.hash(&mut hasher);
            cte.recursive.hash(&mut hasher);
            cte.columns.hash(&mut hasher);
        }

        for window_function in &self.window_functions {
            format!("{:?}", window_function).hash(&mut hasher);
        }

        let hash = hasher.finish();
        crate::cache::QueryCache::global().generate_key(M::table_name(), hash)
    }

    pub async fn get(self) -> Result<Vec<M>> {
        self.ensure_query_is_valid()?;

        let cache_key = if self.cache_options.is_some() {
            let key = self.generate_cache_key();
            if let Some(cached) = crate::cache::QueryCache::global().get::<Vec<M>>(&key) {
                return Ok(cached);
            }
            Some(key)
        } else {
            None
        };

        let (sql, params) = self.build_select_sql_with_params();
        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let results = self
            .current_db()?
            .__raw_with_params::<M>(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context.clone()))?;

        if let (Some(key), Some(options)) = (cache_key, &self.cache_options) {
            let _ = crate::cache::QueryCache::global().set(
                &key,
                &results,
                Some(options.ttl),
                M::table_name(),
            );
        }

        Ok(results)
    }

    pub async fn first(self) -> Result<Option<M>> {
        self.ensure_query_is_valid()?;
        let results = self.limit(1).get().await?;
        Ok(results.into_iter().next())
    }

    pub async fn first_or_fail(self) -> Result<M> {
        self.first()
            .await?
            .ok_or_else(|| Error::not_found(format!("No {} found matching query", M::table_name())))
    }

    pub async fn count(self) -> Result<u64> {
        self.ensure_query_is_valid()?;

        let (sql, params) = self.build_count_sql_with_params();

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows = self
            .current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context.clone()))?;
        let count = rows
            .first()
            .and_then(|row| row.get("count"))
            .map(|value| {
                if let Some(count) = value.as_u64() {
                    Ok(count)
                } else if let Some(count) = value.as_i64() {
                    crate::internal::count_to_u64(count, "query count")
                } else {
                    Ok(0)
                }
            })
            .transpose()?
            .unwrap_or(0);

        Ok(count)
    }

    pub async fn exists(self) -> Result<bool> {
        self.ensure_query_is_valid()?;

        let (sql, params) = self.build_exists_sql_with_params();

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows = self
            .current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context.clone()))?;

        Ok(!rows.is_empty())
    }

    fn ensure_mutation_query_is_safe(&self, operation: &str) -> Result<()> {
        if !self.joins.is_empty()
            || !self.group_by.is_empty()
            || !self.having_conditions.is_empty()
            || !self.unions.is_empty()
            || !self.ctes.is_empty()
            || !self.window_functions.is_empty()
            || self.select_columns.is_some()
            || !self.raw_select_expressions.is_empty()
            || !self.order_by.is_empty()
            || self.limit_value.is_some()
            || self.offset_value.is_some()
        {
            return Err(Error::invalid_query(format!(
                "{} does not support SELECT/JOIN/ORDER/GROUP specific query modifiers",
                operation
            )));
        }

        Ok(())
    }

    fn has_explicit_mutation_filters(&self) -> bool {
        !self.conditions.is_empty()
            || self
                .or_groups
                .iter()
                .any(|group| group.condition_count() > 0)
    }

    fn ensure_mutation_has_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
                "{} requires at least one explicit filter; unfiltered bulk mutations are blocked",
                operation
            )))
        }
    }

    fn ensure_mutation_has_no_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Err(Error::invalid_query(format!(
                "{} does not accept WHERE filters; use delete() when you intend to target specific rows",
                operation
            )))
        } else {
            Ok(())
        }
    }

    fn invalidate_model_cache(rows_affected: u64) {
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
        }
    }

    pub async fn delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("delete")?;
        self.ensure_mutation_has_explicit_filters("delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM {}", table)
        } else {
            format!("DELETE FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_cache(rows_affected);
        Ok(rows_affected)
    }

    /// Delete every row in the table represented by this query.
    ///
    /// This is an explicit opt-in escape hatch for full-table deletion and is kept
    /// separate from `delete()` so accidental unfiltered bulk deletes remain blocked.
    pub async fn delete_all(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("delete_all")?;
        self.ensure_mutation_has_no_explicit_filters("delete_all")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let sql = format!("DELETE FROM {}", table);

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, Vec::new())
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_cache(rows_affected);
        Ok(rows_affected)
    }

    pub async fn soft_delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("soft_delete")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "soft_delete() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("soft_delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
        let now = Self::current_timestamp_sql();
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("UPDATE {} SET {} = {}", table, deleted_at, now)
        } else {
            format!(
                "UPDATE {} SET {} = {} WHERE {}",
                table, deleted_at, now, where_sql
            )
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_cache(rows_affected);
        Ok(rows_affected)
    }

    pub async fn restore(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("restore")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "restore() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("restore")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!(
                "UPDATE {} SET {} = NULL WHERE {} IS NOT NULL",
                table, deleted_at, deleted_at
            )
        } else {
            format!(
                "UPDATE {} SET {} = NULL WHERE {} AND {} IS NOT NULL",
                table, deleted_at, where_sql, deleted_at
            )
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_cache(rows_affected);
        Ok(rows_affected)
    }

    pub async fn force_delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("force_delete")?;
        self.ensure_mutation_has_explicit_filters("force_delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM {}", table)
        } else {
            format!("DELETE FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_cache(rows_affected);
        Ok(rows_affected)
    }

    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        self.ensure_query_is_valid()?;
        let (sql, params) = self.build_select_sql_with_params();
        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(sql.clone()));
        self.current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))
    }
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../testing/query_sql_tests.rs"]
mod tests;
