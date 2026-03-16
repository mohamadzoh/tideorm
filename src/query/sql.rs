use super::*;
use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{Condition, Expr, Value};
use crate::model::Model;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
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
        let db_type = self.db_type_for_sql();
        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
        if where_sql.is_empty() {
            Condition::all()
        } else {
            Condition::all().add(Expr::cust_with_values(where_sql, params))
        }
    }

    fn db_type_for_sql(&self) -> DatabaseType {
        crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres)
    }

    fn current_timestamp_sql(db_type: DatabaseType) -> &'static str {
        match db_type {
            DatabaseType::Postgres | DatabaseType::MySQL | DatabaseType::MariaDB => "CURRENT_TIMESTAMP",
            DatabaseType::SQLite => "CURRENT_TIMESTAMP",
        }
    }

    fn next_placeholder(&self, db_type: DatabaseType, params_len: usize) -> String {
        match db_type {
            DatabaseType::Postgres => format!("${}", params_len + 1),
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => "?".to_string(),
        }
    }

    fn push_param(
        &self,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
        value: &serde_json::Value,
    ) -> String {
        let placeholder = self.next_placeholder(db_type, params.len());
        params.push(Self::json_to_sea_value(value));
        placeholder
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
        if column.contains(' ') {
            column.to_string()
        } else {
            db_sql::format_column(db_type, column)
        }
    }

    fn build_select_clause_sql(&self, db_type: DatabaseType) -> String {
        let table = M::table_name();

        if !self.raw_select_expressions.is_empty() {
            let mut expressions = self.raw_select_expressions.clone();
            for window_function in &self.window_functions {
                expressions.push(window_function.to_sql());
            }
            return format!("SELECT {} ", expressions.join(", "));
        }

        if let Some(columns) = &self.select_columns {
            let mut rendered_columns: Vec<String> = columns
                .iter()
                .map(|column| {
                    if column.contains('(')
                        || column.contains('*')
                        || column.contains('"')
                        || column.contains('`')
                        || column.contains(' ')
                    {
                        column.clone()
                    } else if column.contains('.') {
                        self.format_column_for_db(db_type, column)
                    } else {
                        format!(
                            "{}.{}",
                            db_sql::quote_ident(db_type, table),
                            db_sql::quote_ident(db_type, column)
                        )
                    }
                })
                .collect();

            for window_function in &self.window_functions {
                rendered_columns.push(window_function.to_sql());
            }

            return format!("SELECT {} ", rendered_columns.join(", "));
        }

        let mut select_parts = vec![format!("{}.*", db_sql::quote_ident(db_type, table))];
        for window_function in &self.window_functions {
            select_parts.push(window_function.to_sql());
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
        if matches!(condition.operator, Operator::Raw) {
            return match &condition.value {
                ConditionValue::RawExpr(raw_sql) => {
                    if condition.column.is_empty() {
                        Some(raw_sql.clone())
                    } else {
                        Some(format!(
                            "{} {}",
                            self.format_column_for_db(db_type, &condition.column),
                            raw_sql
                        ))
                    }
                }
                _ => None,
            };
        }

        let column = self.format_column_for_db(db_type, &condition.column);

        match &condition.operator {
            Operator::Eq => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} = {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::NotEq => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} != {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::Gt => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} > {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::Gte => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} >= {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::Lt => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} < {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::Lte => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} <= {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::Like => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} LIKE {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::NotLike => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(format!("{} NOT LIKE {}", column, self.format_preview_value(value)))
                }
                _ => None,
            },
            Operator::In => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered: Vec<String> = values.iter().map(|value| self.format_preview_value(value)).collect();
                    Some(format!("{} IN ({})", column, rendered.join(", ")))
                }
                _ => None,
            },
            Operator::NotIn => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered: Vec<String> = values.iter().map(|value| self.format_preview_value(value)).collect();
                    Some(format!("{} NOT IN ({})", column, rendered.join(", ")))
                }
                _ => None,
            },
            Operator::IsNull => Some(format!("{} IS NULL", column)),
            Operator::IsNotNull => Some(format!("{} IS NOT NULL", column)),
            Operator::Between => match &condition.value {
                ConditionValue::Range(low, high) => Some(format!(
                    "{} BETWEEN {} AND {}",
                    column,
                    self.format_preview_value(low),
                    self.format_preview_value(high)
                )),
                _ => None,
            },
            Operator::JsonContains => match &condition.value {
                ConditionValue::Single(value) => {
                    Some(db_sql::json_contains(db_type, &condition.column, &value.to_string()))
                }
                _ => None,
            },
            Operator::JsonContainedBy => match &condition.value {
                ConditionValue::Single(value) => Some(db_sql::json_contained_by(
                    db_type,
                    &condition.column,
                    &value.to_string(),
                )),
                _ => None,
            },
            Operator::JsonKeyExists => match &condition.value {
                ConditionValue::Single(serde_json::Value::String(key)) => {
                    Some(db_sql::json_key_exists(db_type, &condition.column, key))
                }
                _ => None,
            },
            Operator::JsonKeyNotExists => match &condition.value {
                ConditionValue::Single(serde_json::Value::String(key)) => {
                    Some(db_sql::json_key_not_exists(db_type, &condition.column, key))
                }
                _ => None,
            },
            Operator::JsonPathExists => match &condition.value {
                ConditionValue::Single(serde_json::Value::String(path)) => {
                    Some(db_sql::json_path_exists(db_type, &condition.column, path))
                }
                _ => None,
            },
            Operator::JsonPathNotExists => match &condition.value {
                ConditionValue::Single(serde_json::Value::String(path)) => {
                    Some(db_sql::json_path_not_exists(db_type, &condition.column, path))
                }
                _ => None,
            },
            Operator::ArrayContains | Operator::ArrayContainsAll => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered = self.render_array_values(values);
                    Some(db_sql::array_contains(db_type, &condition.column, &rendered))
                }
                _ => None,
            },
            Operator::ArrayContainedBy => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered = self.render_array_values(values);
                    Some(db_sql::array_contained_by(db_type, &condition.column, &rendered))
                }
                _ => None,
            },
            Operator::ArrayOverlaps | Operator::ArrayContainsAny => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered = self.render_array_values(values);
                    Some(db_sql::array_overlaps(db_type, &condition.column, &rendered))
                }
                _ => None,
            },
            Operator::SubqueryIn => match &condition.value {
                ConditionValue::Subquery(query_sql) => Some(format!("{} IN ({})", column, query_sql)),
                _ => None,
            },
            Operator::SubqueryNotIn => match &condition.value {
                ConditionValue::Subquery(query_sql) => {
                    Some(format!("{} NOT IN ({})", column, query_sql))
                }
                _ => None,
            },
            Operator::EqAny => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered = self.render_array_values(values);
                    Some(db_sql::eq_any(db_type, &column, &rendered))
                }
                _ => None,
            },
            Operator::NeAll => match &condition.value {
                ConditionValue::List(values) => {
                    let rendered = self.render_array_values(values);
                    Some(db_sql::ne_all(db_type, &column, &rendered))
                }
                _ => None,
            },
            Operator::Raw => None,
        }
    }

    fn build_condition_sql_with_params(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
    ) -> Option<String> {
        if matches!(condition.operator, Operator::Raw) {
            return self.build_condition_sql_for_db(condition, db_type);
        }

        let column = self.format_column_for_db(db_type, &condition.column);

        match &condition.operator {
            Operator::Eq => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} = {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::NotEq => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} != {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::Gt => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} > {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::Gte => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} >= {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::Lt => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} < {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::Lte => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} <= {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::Like => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} LIKE {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::NotLike => match &condition.value {
                ConditionValue::Single(value) => Some(format!(
                    "{} NOT LIKE {}",
                    column,
                    self.push_param(db_type, params, value)
                )),
                _ => None,
            },
            Operator::In => match &condition.value {
                ConditionValue::List(values) => {
                    let placeholders: Vec<String> = values
                        .iter()
                        .map(|value| self.push_param(db_type, params, value))
                        .collect();
                    Some(format!("{} IN ({})", column, placeholders.join(", ")))
                }
                _ => None,
            },
            Operator::NotIn => match &condition.value {
                ConditionValue::List(values) => {
                    let placeholders: Vec<String> = values
                        .iter()
                        .map(|value| self.push_param(db_type, params, value))
                        .collect();
                    Some(format!("{} NOT IN ({})", column, placeholders.join(", ")))
                }
                _ => None,
            },
            Operator::IsNull => Some(format!("{} IS NULL", column)),
            Operator::IsNotNull => Some(format!("{} IS NOT NULL", column)),
            Operator::Between => match &condition.value {
                ConditionValue::Range(low, high) => {
                    let low_placeholder = self.push_param(db_type, params, low);
                    let high_placeholder = self.push_param(db_type, params, high);
                    Some(format!("{} BETWEEN {} AND {}", column, low_placeholder, high_placeholder))
                }
                _ => None,
            },
            Operator::EqAny => match &condition.value {
                ConditionValue::List(values) => {
                    let placeholders: Vec<String> = values
                        .iter()
                        .map(|value| self.push_param(db_type, params, value))
                        .collect();
                    Some(db_sql::eq_any(db_type, &column, &placeholders))
                }
                _ => None,
            },
            Operator::NeAll => match &condition.value {
                ConditionValue::List(values) => {
                    let placeholders: Vec<String> = values
                        .iter()
                        .map(|value| self.push_param(db_type, params, value))
                        .collect();
                    Some(db_sql::ne_all(db_type, &column, &placeholders))
                }
                _ => None,
            },
            _ => self.build_condition_sql_for_db(condition, db_type),
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

    fn build_or_group_sql_with_params(
        &self,
        group: &OrGroup,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
    ) -> String {
        let mut parts = Vec::new();

        for condition in &group.conditions {
            if let Some(expression) = self.build_condition_sql_with_params(condition, db_type, params)
            {
                parts.push(expression);
            }
        }

        for nested_group in &group.nested_groups {
            let nested_sql = self.build_or_group_sql_with_params(nested_group, db_type, params);
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
            let deleted_at = db_sql::quote_ident(db_type, "deleted_at");
            if self.only_trashed {
                clauses.push(format!("{} IS NOT NULL", deleted_at));
            } else if !self.include_trashed {
                clauses.push(format!("{} IS NULL", deleted_at));
            }
        }

        clauses.join(" AND ")
    }

    fn build_where_sql_with_params_for_db(&self, db_type: DatabaseType) -> (String, Vec<Value>) {
        let mut clauses = Vec::new();
        let mut params = Vec::new();

        for condition in &self.conditions {
            if let Some(expression) = self.build_condition_sql_with_params(condition, db_type, &mut params)
            {
                clauses.push(expression);
            }
        }

        for group in &self.or_groups {
            let group_sql = self.build_or_group_sql_with_params(group, db_type, &mut params);
            if !group_sql.is_empty() {
                clauses.push(format!("({})", group_sql));
            }
        }

        if M::soft_delete_enabled() {
            let deleted_at = db_sql::quote_ident(db_type, "deleted_at");
            if self.only_trashed {
                clauses.push(format!("{} IS NOT NULL", deleted_at));
            } else if !self.include_trashed {
                clauses.push(format!("{} IS NULL", deleted_at));
            }
        }

        (clauses.join(" AND "), params)
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

    fn build_base_select_sql_with_params_for_db(&self, db_type: DatabaseType) -> (String, Vec<Value>) {
        let mut sql = String::new();

        sql.push_str(&self.build_select_clause_sql(db_type));
        self.append_from_and_join_sql(&mut sql, db_type);

        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
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
            sql.push_str(if recursive { "WITH RECURSIVE " } else { "WITH " });
            let cte_parts: Vec<String> = self.ctes.iter().map(CTE::to_sql).collect();
            sql.push_str(&cte_parts.join(", "));
            sql.push(' ');
        }

        sql.push_str(&self.build_base_select_sql());

        for union in &self.unions {
            sql.push_str(&format!(" {} {}", union.union_type.as_sql(), union.query_sql));
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

    pub(crate) fn build_select_sql_with_params_for_db(&self, db_type: DatabaseType) -> (String, Vec<Value>) {
        let mut sql = String::new();

        if !self.ctes.is_empty() {
            let recursive = self.ctes.iter().any(|cte| cte.recursive);
            sql.push_str(if recursive { "WITH RECURSIVE " } else { "WITH " });
            let cte_parts: Vec<String> = self.ctes.iter().map(CTE::to_sql).collect();
            sql.push_str(&cte_parts.join(", "));
            sql.push(' ');
        }

        let (base_sql, params) = self.build_base_select_sql_with_params_for_db(db_type);
        sql.push_str(&base_sql);

        for union in &self.unions {
            sql.push_str(&format!(" {} ({})", union.union_type.as_sql(), union.query_sql));
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

    fn log_query(&self, sql: &str) {
        if std::env::var("TIDE_LOG_QUERIES")
            .map(|value| value.to_ascii_lowercase() == "true" || value == "1")
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
        let mut info = QueryDebugInfo::new(M::table_name()).with_sql(self.build_sql_preview());
        info.params = params.into_iter().map(|value| format!("{:?}", value)).collect();

        for condition in &self.conditions {
            let operator = match condition.operator {
                Operator::Eq => "=",
                Operator::NotEq => "!=",
                Operator::Gt => ">",
                Operator::Gte => ">=",
                Operator::Lt => "<",
                Operator::Lte => "<=",
                Operator::Like => "LIKE",
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
            };

            let value = match &condition.value {
                ConditionValue::Single(value) => value.to_string(),
                ConditionValue::List(values) => format!("{:?}", values),
                ConditionValue::Range(low, high) => format!("{}..{}", low, high),
                ConditionValue::None => "NULL".to_string(),
                ConditionValue::Subquery(query_sql) => query_sql.clone(),
                ConditionValue::RawExpr(raw_sql) => raw_sql.clone(),
            };

            info.add_condition(format!("{} {} {}", condition.column, operator, value));
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
            info.sql = parameterized_sql;
        }

        info
    }

    pub fn build_sql_preview(&self) -> String {
        self.build_select_sql()
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
        let results = crate::database::Database::raw_with_params::<M>(&sql, params).await?;

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
        self.first().await?.ok_or_else(|| {
            Error::not_found(format!("No {} found matching query", M::table_name()))
        })
    }

    pub async fn count(self) -> Result<u64> {
        self.ensure_query_is_valid()?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("SELECT COUNT(*) AS count FROM {}", table)
        } else {
            format!("SELECT COUNT(*) AS count FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        let rows = crate::database::Database::raw_json_with_params(&sql, params).await?;
        let count = rows
            .first()
            .and_then(|row| row.get("count"))
            .and_then(|value| value.as_u64().or_else(|| value.as_i64().map(|number| number as u64)))
            .unwrap_or(0);

        Ok(count)
    }

    pub async fn exists(self) -> Result<bool> {
        Ok(self.count().await? > 0)
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

    pub async fn delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM {}", table)
        } else {
            format!("DELETE FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        crate::database::Database::execute_with_params(&sql, params).await
    }

    pub async fn soft_delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("soft_delete")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "soft_delete() can only be used on models with soft delete enabled",
            ));
        }

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, "deleted_at");
        let now = Self::current_timestamp_sql(db_type);
        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("UPDATE {} SET {} = {}", table, deleted_at, now)
        } else {
            format!("UPDATE {} SET {} = {} WHERE {}", table, deleted_at, now, where_sql)
        };

        self.log_query(&sql);
        crate::database::Database::execute_with_params(&sql, params).await
    }

    pub async fn restore(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("restore")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "restore() can only be used on models with soft delete enabled",
            ));
        }

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, "deleted_at");
        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
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
        crate::database::Database::execute_with_params(&sql, params).await
    }

    pub async fn force_delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("force_delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_sql_with_params_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM {}", table)
        } else {
            format!("DELETE FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        crate::database::Database::execute_with_params(&sql, params).await
    }

    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        self.ensure_query_is_valid()?;
        let (sql, params) = self.build_select_sql_with_params();
        self.log_query(&sql);
        crate::database::Database::raw_json_with_params(&sql, params).await
    }
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}
